//! Bounded JSONL transport. Managed workers journal sanitized frames before exposing receipts.
use super::*;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    io::{BufRead, BufReader, Read, Write},
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    time::{Duration, Instant},
};

const FRAME_LIMIT: usize = 1024 * 1024;
const RESUME_REPLY_LIMIT: usize = 16 * FRAME_LIMIT;

#[derive(serde::Deserialize)]
struct ReplyHeader {
    id: Option<u64>,
    method: Option<String>,
}

struct Journal {
    file: Mutex<File>,
    secrets: Vec<String>,
    private_requests: Mutex<HashSet<u64>>,
    stream_tails: Mutex<HashMap<String, String>>,
}
impl Journal {
    fn sanitize(&self, value: &Value, parent: &str) -> Value {
        match value {
            Value::Object(map) => Value::Object(
                map.iter()
                    .map(|(key, value)| {
                        let lower = key.to_ascii_lowercase();
                        let sensitive = [
                            "apikey",
                            "api_key",
                            "access_token",
                            "refresh_token",
                            "id_token",
                            "authorization",
                            "accesstoken",
                            "refreshtoken",
                            "idtoken",
                            "password",
                            "secret",
                            "token",
                            "authurl",
                            "verificationurl",
                        ]
                        .contains(&lower.as_str())
                            || (parent == "error" && lower == "message");
                        (
                            key.clone(),
                            if sensitive {
                                json!("[redacted]")
                            } else {
                                self.sanitize(value, key)
                            },
                        )
                    })
                    .collect(),
            ),
            Value::Array(items) => {
                Value::Array(items.iter().map(|v| self.sanitize(v, parent)).collect())
            }
            Value::String(text) => {
                let mut text = text.clone();
                for secret in &self.secrets {
                    if !secret.is_empty() {
                        text = text.replace(secret, "[redacted]");
                    }
                }
                Value::String(text)
            }
            other => other.clone(),
        }
    }
    fn inbound(&self, frame: Value) -> Result<Option<Value>> {
        let method = frame["method"].as_str().unwrap_or("");
        if method == "item/agentMessage/delta" {
            let p = &frame["params"];
            let key = serde_json::to_string(&json!([p["threadId"], p["turnId"], p["itemId"]]))
                .map_err(storage)?;
            let mut tails = self.stream_tails.lock().map_err(storage)?;
            require(
                tails.len() < 64 || tails.contains_key(&key),
                "流式项目超过限制",
            )?;
            let raw = tails.entry(key).or_default();
            raw.push_str(p["delta"].as_str().unwrap_or(""));
            let retain = self.secrets.iter().map(|s| s.len()).max().unwrap_or(0);
            let mut cut = raw.len().saturating_sub(retain);
            while !raw.is_char_boundary(cut) {
                cut -= 1;
            }
            for secret in self.secrets.iter().filter(|s| !s.is_empty()) {
                for (start, _) in raw.match_indices(secret) {
                    if start < cut && start + secret.len() > cut {
                        cut = start;
                    }
                }
            }
            if cut == 0 {
                return Ok(None);
            }
            let text = raw[..cut].to_string();
            *raw = raw[cut..].to_string();
            drop(tails);
            let mut clean = self.sanitize(&frame, "");
            clean["params"]["delta"] = self.sanitize(&json!(text), "delta");
            return Ok(Some(clean));
        }
        if method == "item/completed" {
            let p = &frame["params"];
            let key = serde_json::to_string(&json!([p["threadId"], p["turnId"], p["item"]["id"]]))
                .map_err(storage)?;
            self.stream_tails.lock().map_err(storage)?.remove(&key);
        }
        let mut clean = self.sanitize(&frame, "");
        if clean["params"]["delta"].is_string() {
            clean["params"]["delta"] =
                json!("[tool stream delta omitted; sanitized final item retained]");
        }
        Ok(Some(clean))
    }
    fn append(&self, direction: &str, frame: &Value) -> Result<()> {
        let private_method = frame["method"]
            .as_str()
            .is_some_and(|m| m.starts_with("account/") || m.starts_with("auth/"));
        if direction == "out" && private_method {
            if let Some(id) = frame["id"].as_u64() {
                self.private_requests.lock().map_err(storage)?.insert(id);
            }
            return Ok(());
        }
        if private_method {
            return Ok(());
        }
        if direction == "in"
            && frame["method"].is_null()
            && frame["id"].as_u64().is_some_and(|id| {
                self.private_requests
                    .lock()
                    .is_ok_and(|mut set| set.remove(&id))
            })
        {
            return Ok(());
        }
        let mut frame = frame.clone();
        // Raw deltas can split a secret across frames. Retain their boundaries, not reconstructable text.
        if frame["params"]["delta"].is_string() {
            frame["params"]["delta"] =
                json!("[stream delta omitted; sanitized final item retained]");
        }
        if frame["params"]["event"]["type"]
            .as_str()
            .is_some_and(|t| t.ends_with("/delta"))
        {
            frame["params"]["event"]["data"] = json!({"omitted":"stream delta"});
        }
        let frame = self.sanitize(&frame, "");
        let mut bytes =
            serde_json::to_vec(&json!({"direction":direction,"frame":frame})).map_err(storage)?;
        bytes.push(b'\n');
        let mut file = self.file.lock().map_err(storage)?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_data())
            .map_err(storage)
    }
}

type WriteRequest = (Vec<u8>, SyncSender<bool>);

#[derive(Default)]
struct AgentMessageBatch {
    pending: Option<Value>,
}
impl AgentMessageBatch {
    fn push(&mut self, frame: Value) -> Vec<Value> {
        let delta = (frame["method"] == "item/agentMessage/delta")
            .then(|| frame["params"]["delta"].as_str())
            .flatten();
        if let (Some(pending), Some(delta)) = (self.pending.as_mut(), delta) {
            let same_item = ["threadId", "turnId", "itemId"]
                .iter()
                .all(|key| pending["params"][key] == frame["params"][key]);
            if same_item {
                if let Value::String(text) = &mut pending["params"]["delta"] {
                    if text.len() + delta.len() <= 4096 {
                        text.push_str(delta);
                        return vec![];
                    }
                }
            }
        }
        let mut ready: Vec<_> = self.pending.take().into_iter().collect();
        if delta.is_some() {
            self.pending = Some(frame);
        } else {
            ready.push(frame);
        }
        ready
    }
    fn finish(&mut self) -> Option<Value> {
        self.pending.take()
    }
}

pub(crate) struct JsonlProcess {
    child: Child,
    input: Option<SyncSender<WriteRequest>>,
    reader: Option<std::thread::JoinHandle<()>>,
    writer: Option<std::thread::JoinHandle<()>>,
    frames: Receiver<(Value, usize)>,
    queued: Arc<AtomicUsize>,
    resume_replies: Arc<Mutex<HashSet<u64>>>,
    failed: Arc<AtomicBool>,
    next: u64,
    journal: Option<Arc<Journal>>,
    capture: bool,
    pending: VecDeque<Value>,
    rpc2: bool,
    pub login_completed: Option<(String, bool)>,
}
impl JsonlProcess {
    pub fn spawn(command: Command) -> Result<Self> {
        Self::spawn_inner(command, None, vec![])
    }
    pub fn managed(command: Command, journal: &Path, secrets: Vec<String>) -> Result<Self> {
        Self::spawn_inner(command, Some(journal), secrets)
    }
    fn spawn_inner(
        mut command: Command,
        path: Option<&Path>,
        secrets: Vec<String>,
    ) -> Result<Self> {
        let journal = if let Some(path) = path {
            reject_symlink(path)?;
            let mut options = OpenOptions::new();
            options.append(true).create(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            Some(Arc::new(Journal {
                file: Mutex::new(options.open(path).map_err(storage)?),
                secrets,
                private_requests: Mutex::new(HashSet::new()),
                stream_tails: Mutex::new(HashMap::new()),
            }))
        } else {
            None
        };
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .env_remove("OPENAI_API_KEY")
            .env_remove("DEEPSEEK_API_KEY")
            .env_remove("ZAI_CODING_CN_API_KEY")
            .env_remove("MINIMAX_CN_API_KEY")
            .env_remove("ZAI_API_KEY")
            .env_remove("MINIMAX_API_KEY")
            .env_remove("OLLAMA_API_KEY")
            .env_remove("ANTHROPIC_API_KEY")
            .env_remove("ANTHROPIC_AUTH_TOKEN")
            .spawn()
            .map_err(|_| err("provider_unavailable", "无法启动协议 worker"))?;
        let mut stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let failed = Arc::new(AtomicBool::new(false));
        let (input, writes) = mpsc::sync_channel::<WriteRequest>(8);
        let signal = failed.clone();
        let writer = std::thread::spawn(move || {
            while let Ok((bytes, receipt)) = writes.recv() {
                let ok = stdin.write_all(&bytes).and_then(|_| stdin.flush()).is_ok();
                let _ = receipt.send(ok);
                if !ok {
                    signal.store(true, Ordering::Release);
                    break;
                }
            }
        });
        let (tx, frames) = mpsc::sync_channel(4096);
        let queued = Arc::new(AtomicUsize::new(0));
        let resume_replies = Arc::new(Mutex::new(HashSet::<u64>::new()));
        let expected_resumes = resume_replies.clone();
        let budget = queued.clone();
        let signal = failed.clone();
        let log = journal.clone();
        let reader = std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut batch = AgentMessageBatch::default();
            let enqueue = |frame: Value| {
                let cost = serde_json::to_vec(&frame)
                    .map(|bytes| bytes.len().saturating_mul(4))
                    .unwrap_or(8 * 1024 * 1024);
                if budget
                    .fetch_add(cost, Ordering::AcqRel)
                    .saturating_add(cost)
                    > 8 * 1024 * 1024
                {
                    budget.fetch_sub(cost, Ordering::AcqRel);
                    signal.store(true, Ordering::Release);
                    return false;
                }
                if tx.try_send((frame, cost)).is_err() {
                    budget.fetch_sub(cost, Ordering::AcqRel);
                    signal.store(true, Ordering::Release);
                    return false;
                }
                true
            };
            loop {
                let mut bytes = Vec::new();
                let read = reader
                    .by_ref()
                    .take(RESUME_REPLY_LIMIT as u64 + 1)
                    .read_until(b'\n', &mut bytes);
                if !matches!(read, Ok(1..)) {
                    break;
                }
                if bytes.len() > RESUME_REPLY_LIMIT {
                    signal.store(true, Ordering::Release);
                    break;
                }
                if bytes.len() > FRAME_LIMIT {
                    // Read only the envelope before allocating a large Value.
                    // Only our outstanding resume metadata reply gets this limit.
                    let allowed = serde_json::from_slice::<ReplyHeader>(&bytes)
                        .ok()
                        .filter(|h| h.method.is_none())
                        .and_then(|h| h.id)
                        .is_some_and(|id| {
                            expected_resumes.lock().is_ok_and(|ids| ids.contains(&id))
                        });
                    if !allowed {
                        signal.store(true, Ordering::Release);
                        break;
                    }
                }
                let frame = match serde_json::from_slice::<Value>(&bytes) {
                    Ok(frame) => frame,
                    Err(_) => {
                        signal.store(true, Ordering::Release);
                        break;
                    }
                };
                let resume_reply = frame.get("method").is_none()
                    && frame["id"].as_u64().is_some_and(|id| {
                        expected_resumes.lock().is_ok_and(|ids| ids.contains(&id))
                    });
                if bytes.len() > FRAME_LIMIT && !resume_reply {
                    signal.store(true, Ordering::Release);
                    break;
                }
                if log
                    .as_ref()
                    .is_some_and(|log| log.append("in", &frame).is_err())
                {
                    signal.store(true, Ordering::Release);
                    break;
                }
                let mut frame = if let Some(log) = &log {
                    match log.inbound(frame) {
                        Ok(Some(frame)) => frame,
                        Ok(None) => continue,
                        Err(_) => {
                            signal.store(true, Ordering::Release);
                            break;
                        }
                    }
                } else {
                    frame
                };
                if resume_reply {
                    if let Some(id) = frame["id"].as_u64() {
                        if let Ok(mut ids) = expected_resumes.lock() {
                            ids.remove(&id);
                        }
                    }
                    // The provider restores its history itself. Our caller needs
                    // thread metadata, not another queued copy of every old turn.
                    // The complete sanitized response was journaled above.
                    if let Some(thread) = frame
                        .pointer_mut("/result/thread")
                        .and_then(Value::as_object_mut)
                    {
                        thread.remove("turns");
                    }
                }
                // Keep bounded memory while avoiding a frame-count overflow when a
                // valid evidence delivery arrives as thousands of tiny text deltas.
                // Journal/sanitize first, and flush before every other frame so
                // terminal receipts and interleaved items keep their exact order.
                for frame in batch.push(frame) {
                    if !enqueue(frame) {
                        return;
                    }
                }
            }
            if !signal.load(Ordering::Acquire) {
                if let Some(frame) = batch.finish() {
                    enqueue(frame);
                }
            }
        });
        Ok(Self {
            child,
            input: Some(input),
            reader: Some(reader),
            writer: Some(writer),
            frames,
            queued,
            resume_replies,
            failed,
            next: 1,
            capture: journal.is_some(),
            journal,
            pending: VecDeque::new(),
            rpc2: false,
            login_completed: None,
        })
    }
    pub fn rpc2(&mut self) {
        self.rpc2 = true;
    }
    pub fn send(&mut self, value: &Value) -> Result<()> {
        require(!self.failed.load(Ordering::Acquire), "协议连接已失效")?;
        let mut value = value.clone();
        if self.rpc2 {
            value["jsonrpc"] = json!("2.0");
        }
        if let Some(log) = &self.journal {
            log.append("out", &value)?;
        }
        let mut bytes = serde_json::to_vec(&value).map_err(storage)?;
        require(bytes.len() <= 1024 * 1024, "协议输入超出限制")?;
        bytes.push(b'\n');
        let (tx, receipt) = mpsc::sync_channel(1);
        self.input
            .as_ref()
            .ok_or_else(|| err("needs_reconciliation", "协议已关闭"))?
            .try_send((bytes, tx))
            .map_err(|_| err("needs_reconciliation", "协议发送队列不可用；禁止重发输入"))?;
        match receipt.recv_timeout(Duration::from_secs(5)) {
            Ok(true) => Ok(()),
            _ => Err(err(
                "needs_reconciliation",
                "协议写入结果未知；禁止重发输入",
            )),
        }
    }
    pub fn submit(&mut self, method: &str, params: Value) -> Result<u64> {
        let id = self.next;
        self.next += 1;
        if method == "thread/resume" {
            self.resume_replies.lock().map_err(storage)?.insert(id);
        }
        if let Err(error) = self.send(&json!({"id":id,"method":method,"params":params})) {
            self.resume_replies.lock().map_err(storage)?.remove(&id);
            return Err(error);
        }
        Ok(id)
    }
    pub fn initialize_codex(&mut self) -> Result<()> {
        self.request(
            "initialize",
            json!({"clientInfo":{"name":"devdesk","title":"DevDesk","version":"0.2.0"},"capabilities":{"experimentalApi":true}}),
        )?;
        self.send(&json!({"method":"initialized","params":{}}))
    }
    pub fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        self.request_for(method, params, Duration::from_secs(20))
    }
    pub fn request_for(&mut self, method: &str, params: Value, timeout: Duration) -> Result<Value> {
        let id = self.submit(method, params)?;
        let deadline = Instant::now() + timeout;
        loop {
            require(
                !self.failed.load(Ordering::Acquire),
                "协议帧无效、落盘失败或事件缓冲溢出",
            )?;
            let (frame, cost) = self
                .frames
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .map_err(|_| {
                    err(
                        "needs_reconciliation",
                        format!("协议调用 {method} 超时或协议通道关闭；禁止重复派发"),
                    )
                })?;
            self.queued.fetch_sub(cost, Ordering::AcqRel);
            if frame["method"] == "account/login/completed" {
                if let (Some(login), Some(success)) = (
                    frame["params"]["loginId"].as_str(),
                    frame["params"]["success"].as_bool(),
                ) {
                    self.login_completed = Some((login.into(), success));
                }
            }
            if frame["id"] == id && frame.get("method").is_none() {
                if frame.get("error").is_some() {
                    return Err(err("provider_failure", "运行时拒绝请求，请核对协议与认证"));
                }
                return frame
                    .get("result")
                    .cloned()
                    .ok_or_else(|| err("provider_io", "协议响应缺少结果"));
            }
            if self.capture {
                require(self.pending.len() < 256, "初始化事件超出限制")?;
                self.pending.push_back(frame);
            } else if frame.get("method").is_some() && frame.get("id").is_some() {
                self.send(&json!({"id":frame["id"],"error":{"code":-32601,"message":"Authentication worker does not execute tools"}}))?;
            }
            require(Instant::now() < deadline, "协议调用超时")?;
        }
    }
    pub fn drain(&mut self) -> Result<Vec<Value>> {
        require(
            !self.failed.load(Ordering::Acquire),
            "协议失败或帧日志不能落盘",
        )?;
        let mut frames: Vec<_> = self.pending.drain(..).collect();
        for _ in 0..256 {
            match self.frames.try_recv() {
                Ok((frame, cost)) => {
                    self.queued.fetch_sub(cost, Ordering::AcqRel);
                    frames.push(frame)
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    if frames.is_empty() {
                        return Err(err("needs_reconciliation", "协议 worker 已退出"));
                    }
                    break;
                }
            }
        }
        Ok(frames)
    }
}
impl Drop for JsonlProcess {
    fn drop(&mut self) {
        // After the coordinator stops the container, allow its attach process to drain stdout.
        let deadline = Instant::now() + Duration::from_secs(1);
        while self.child.try_wait().is_ok_and(|s| s.is_none()) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        crate::process::kill_group(&mut self.child);
        let _ = self.child.wait();
        self.input.take();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
        if let Some(writer) = self.writer.take() {
            let _ = writer.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resume_restores_large_history_without_requeueing_it_or_losing_the_journal() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let secret = "HARMLESS_RESUME_SECRET";
        let mut command = Command::new("node");
        command.args(["-e", "require('node:readline').createInterface({input:process.stdin}).on('line',line=>{const q=JSON.parse(line);if(q.method==='thread/resume'){console.log(JSON.stringify({id:q.id,result:{thread:{id:'retained-thread',turns:[{id:'old',text:'x'.repeat(1200000),token:'HARMLESS_RESUME_SECRET'}]}}}));console.log(JSON.stringify({method:'turn/completed',params:{turn:{id:'last',status:'completed'}}}));}else{console.log(JSON.stringify({id:q.id,result:{ok:true}}));}});"]);
        let mut wire = JsonlProcess::managed(command, file.path(), vec![secret.into()]).unwrap();
        let result = wire
            .request("thread/resume", json!({"threadId":"retained-thread"}))
            .unwrap();
        assert_eq!(result["thread"]["id"], "retained-thread");
        assert!(result["thread"].get("turns").is_none());
        assert!(wire.resume_replies.lock().unwrap().is_empty());
        assert_eq!(wire.request("ping", json!({})).unwrap()["ok"], true);
        assert!(
            wire.drain()
                .unwrap()
                .iter()
                .any(|frame| frame["method"] == "turn/completed")
        );
        let text = fs::read_to_string(file.path()).unwrap();
        assert!(!text.contains(secret));
        let response: Value = text
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .find(|entry| entry["frame"]["result"]["thread"]["id"] == "retained-thread")
            .unwrap();
        assert_eq!(
            response["frame"]["result"]["thread"]["turns"][0]["text"]
                .as_str()
                .unwrap()
                .len(),
            1200000
        );
        assert!(!wire.failed.load(Ordering::Acquire));
    }

    #[test]
    fn only_the_registered_resume_reply_can_exceed_the_normal_frame_limit() {
        for mode in ["ordinary", "notification", "too_large"] {
            let mut command = Command::new("node");
            command.args(["-e", "const mode=process.argv[1];require('node:readline').createInterface({input:process.stdin}).on('line',line=>{const q=JSON.parse(line),text='x'.repeat(mode==='too_large'?17000000:1200000);console.log(JSON.stringify(mode==='notification'?{id:q.id,method:'unexpected',params:{text}}:{id:q.id,result:{thread:{id:'t',turns:[{text}]}}}));});", mode]);
            let mut wire = JsonlProcess::spawn(command).unwrap();
            let method = if mode == "ordinary" {
                "models/list"
            } else {
                "thread/resume"
            };
            assert!(wire.request(method, json!({})).is_err(), "{mode}");
            assert!(wire.failed.load(Ordering::Acquire), "{mode}");
        }
    }

    #[test]
    fn evidence_text_bursts_preserve_full_content_and_terminal_receipts() {
        let mut command = Command::new("node");
        command.args(["-e", "for(let i=0;i<9000;i++) console.log(JSON.stringify({method:'item/agentMessage/delta',params:{threadId:'t',turnId:'u',itemId:'i',delta:'x'}})); console.log(JSON.stringify({method:'item/completed',params:{item:{id:'i',type:'agentMessage',text:'x'.repeat(9000)}}})); console.log(JSON.stringify({method:'turn/completed',params:{turn:{id:'u',status:'completed'}}}));"]);
        let mut wire = JsonlProcess::spawn(command).unwrap();
        // Deliberately let the provider finish before consuming any frames.
        let deadline = Instant::now() + Duration::from_secs(10);
        while wire.child.try_wait().unwrap().is_none() {
            assert!(Instant::now() < deadline, "provider did not finish");
            std::thread::sleep(Duration::from_millis(5));
        }
        wire.reader.take().unwrap().join().unwrap();
        assert!(!wire.failed.load(Ordering::Acquire));
        let frames = wire.drain().unwrap();
        let text: String = frames
            .iter()
            .filter(|frame| frame["method"] == "item/agentMessage/delta")
            .map(|frame| frame["params"]["delta"].as_str().unwrap())
            .collect();
        assert_eq!(text, "x".repeat(9000));
        assert_eq!(frames[frames.len() - 2]["params"]["item"]["text"], text);
        assert_eq!(frames.last().unwrap()["method"], "turn/completed");
        assert_eq!(wire.queued.load(Ordering::Acquire), 0);
    }

    #[test]
    fn text_batches_never_cross_other_events_or_message_identities() {
        let delta = |item: &str, text: &str| json!({"method":"item/agentMessage/delta","params":{"threadId":"t","turnId":"u","itemId":item,"delta":text}});
        let mut batch = AgentMessageBatch::default();
        assert!(batch.push(delta("first", "a")).is_empty());
        assert!(batch.push(delta("first", "b")).is_empty());
        let event = json!({"method":"item/started","params":{"item":{"id":"second"}}});
        assert_eq!(batch.push(event.clone()), vec![delta("first", "ab"), event]);
        assert!(batch.push(delta("second", "x")).is_empty());
        assert_eq!(batch.push(delta("first", "y")), vec![delta("second", "x")]);
        assert_eq!(batch.finish(), Some(delta("first", "y")));
        assert_eq!(batch.finish(), None);
    }

    #[test]
    fn split_secret_deltas_never_become_reconstructable_logs_or_ui_events() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let secret = "HARMLESS_SECRET_CANARY";
        let journal = Journal {
            file: Mutex::new(temp.reopen().unwrap()),
            secrets: vec![secret.into()],
            private_requests: Mutex::new(HashSet::new()),
            stream_tails: Mutex::new(HashMap::new()),
        };
        let mut text = String::new();
        for chunk in [
            "safe prelude ",
            "HARMLESS_SE",
            "CRET_CANARY",
            " safe epilogue repeated repeated repeated",
        ] {
            let frame = json!({"method":"item/agentMessage/delta","params":{"threadId":"t","turnId":"u","itemId":"i","delta":chunk}});
            journal.append("in", &frame).unwrap();
            if let Some(clean) = journal.inbound(frame).unwrap() {
                text.push_str(clean["params"]["delta"].as_str().unwrap());
            }
        }
        let final_frame = json!({"method":"item/completed","params":{"threadId":"t","turnId":"u","item":{"id":"i","type":"agentMessage","text":format!("safe prelude {secret} safe epilogue repeated repeated repeated")}}});
        journal.append("in", &final_frame).unwrap();
        let clean = journal.inbound(final_frame).unwrap().unwrap();
        assert!(
            clean["params"]["item"]["text"]
                .as_str()
                .unwrap()
                .contains("[redacted]")
        );
        assert!(!text.contains(secret));
        let log = fs::read_to_string(temp.path()).unwrap();
        assert!(!log.contains(secret));
        assert!(!log.contains("HARMLESS_SE"));
        assert!(!log.contains("CRET_CANARY"));
        assert!(journal.stream_tails.lock().unwrap().is_empty());
    }
}
