// Bounded, consumer-owned requests. Cancelling a consumer does not cancel another consumer's work.
export class QueryCache {
  private entries = new Map<
    string,
    {
      controller: AbortController;
      promise: Promise<unknown>;
      consumers: number;
      value?: unknown;
      expires: number;
      bytes: number;
    }
  >();
  private bytes = 0;
  private readonly maxEntries: number;
  private readonly maxBytes: number;
  private readonly retainPending: boolean;
  constructor(
    maxEntries = 64,
    maxBytes = 8 * 1024 * 1024,
    retainPending = false,
  ) {
    this.maxEntries = maxEntries;
    this.maxBytes = maxBytes;
    this.retainPending = retainPending;
  }
  peek<T>(key: string): T | undefined {
    return this.entries.get(key)?.value as T | undefined;
  }
  clear() {
    for (const entry of this.entries.values()) entry.controller.abort();
    this.entries.clear();
    this.bytes = 0;
  }
  read<T>(
    key: string,
    load: (signal: AbortSignal) => Promise<T>,
    signal?: AbortSignal,
    ttl = Infinity,
  ): Promise<T> {
    if (signal?.aborted)
      return Promise.reject(new DOMException("Aborted", "AbortError"));
    let entry = this.entries.get(key);
    if (entry && entry.expires <= Date.now()) {
      this.bytes -= entry.bytes;
      this.entries.delete(key);
      entry = undefined;
    }
    if (!entry) {
      if (this.entries.size >= this.maxEntries) {
        const old = [...this.entries].find(
          ([, value]) => value.value !== undefined,
        );
        if (old) {
          this.entries.delete(old[0]);
          this.bytes -= old[1].bytes;
        } else if (this.retainPending)
          return Promise.reject(new Error("后台模型目录繁忙，请稍后重试"));
        else return load(signal || new AbortController().signal);
      }
      const controller = new AbortController();
      entry = {
        controller,
        promise: Promise.resolve(),
        consumers: 0,
        expires: Infinity,
        bytes: 0,
      };
      const current = entry;
      current.promise = Promise.resolve()
        .then(() => load(controller.signal))
        .then((value) => {
          if (controller.signal.aborted || this.entries.get(key) !== current)
            throw new DOMException("Aborted", "AbortError");
          const bytes =
            new TextEncoder().encode(JSON.stringify(value)).length * 2;
          current.value = value;
          current.expires = Date.now() + ttl;
          current.bytes = bytes;
          this.bytes += bytes;
          this.prune(key);
          return value;
        })
        .catch((error) => {
          if (this.entries.get(key) === current) {
            this.bytes -= current.bytes;
            this.entries.delete(key);
          }
          throw error;
        });
      this.entries.set(key, current);
    }
    // Map order is LRU; unresolved entries with consumers are never evicted.
    this.entries.delete(key);
    this.entries.set(key, entry);
    const current = entry;
    current.consumers += 1;
    return new Promise<T>((resolve, reject) => {
      let done = false;
      const finish = () => {
        if (done) return false;
        done = true;
        signal?.removeEventListener("abort", abort);
        current.consumers -= 1;
        if (
          !this.retainPending &&
          !current.consumers &&
          current.value === undefined
        ) {
          current.controller.abort();
          if (this.entries.get(key) === current) this.entries.delete(key);
        }
        return true;
      };
      const abort = () => {
        if (finish()) reject(new DOMException("Aborted", "AbortError"));
      };
      signal?.addEventListener("abort", abort, { once: true });
      current.promise.then(
        (value) => {
          if (finish()) resolve(value as T);
        },
        (error) => {
          if (finish()) reject(error);
        },
      );
    });
  }
  private prune(keep: string) {
    for (const [key, entry] of this.entries) {
      if (this.entries.size <= this.maxEntries && this.bytes <= this.maxBytes)
        break;
      if (key !== keep && entry.value !== undefined) {
        this.entries.delete(key);
        this.bytes -= entry.bytes;
      }
    }
    const entry = this.entries.get(keep);
    if (entry && entry.bytes > this.maxBytes) {
      this.entries.delete(keep);
      this.bytes -= entry.bytes;
    }
  }
}
