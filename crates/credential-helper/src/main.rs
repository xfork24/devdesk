//! Immutable per-user Keychain principal. Host rebuilds do not rebuild this installation.
use serde::Deserialize;
use serde_json::{Value, json};
use std::io::{Read, Write};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    operation: String,
    service: String,
    account: String,
    secret: Option<String>,
}
fn valid_account(account: &str) -> bool {
    account == "deepseek-official-api-key" || ["deepseek-api-key-", "dsh-zai-coding-cn-api-key-", "dsh-minimax-cn-api-key-"].iter().any(|prefix| {
        account.strip_prefix(prefix).is_some_and(|v| v.len()==36 && v.bytes().enumerate().all(|(i,c)|if [8,13,18,23].contains(&i){c==b'-'}else{c.is_ascii_hexdigit()}))
    })
}
fn validate(r: &Request) -> bool {
    r.service.strip_prefix("devdesk.").is_some_and(|s| {
        s.len() == 64
            && s.bytes()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    }) && valid_account(&r.account)
        && ["status", "store", "read", "check", "authorize"].contains(&r.operation.as_str())
        && if r.operation == "store" {
            r.secret.as_ref().is_some_and(|s| {
                (8..=4096).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_graphic())
            })
        } else {
            r.secret.is_none()
        }
}
#[cfg(target_os = "macos")]
fn authorized_parent() -> bool {
    use security_framework::os::macos::code_signing::{
        Flags, GuestAttributes, SecCode, SecRequirement,
    };
    let certificate = option_env!("DEVDESK_ALLOWED_HOST_CERTIFICATE").unwrap_or("");
    if certificate.len() != 40 || !certificate.bytes().all(|c| c.is_ascii_hexdigit()) {
        return false;
    }
    let Ok(requirement) =
        format!("identifier \"dev.devdesk.host\" and certificate leaf = H\"{certificate}\"")
            .parse::<SecRequirement>()
    else {
        return false;
    };
    let mut attrs = GuestAttributes::new();
    attrs.set_pid(unsafe { libc::getppid() });
    SecCode::copy_guest_with_attribues(None, &attrs, Flags::empty())
        .is_ok_and(|parent| parent.check_validity(Flags::empty(), &requirement).is_ok())
}
#[cfg(target_os = "macos")]
fn execute(r: &mut Request) -> Result<Value, security_framework::base::Error> {
    use security_framework::{os::macos::keychain::SecKeychain, passwords};
    // Each invocation is single-threaded. Interactive access exists only for the
    // explicit authorization operation; normal Host polling can never display UI.
    let _restore = if r.operation != "authorize" {
        Some(SecKeychain::disable_user_interaction()?)
    } else {
        None
    };
    match r.operation.as_str() {
        "status" => {
            use security_framework::item::{ItemClass, ItemSearchOptions};
            match ItemSearchOptions::new()
                .class(ItemClass::generic_password())
                .service(&r.service)
                .account(&r.account)
                .load_attributes(true)
                .search()
            {
                Ok(items) => Ok(json!({"configured":!items.is_empty()})),
                Err(e) if e.code() == -25300 => Ok(json!({"configured":false})),
                Err(e) => Err(e),
            }
        }
        "store" => {
            let mut secret = r.secret.take().unwrap().into_bytes();
            let result = passwords::set_generic_password(&r.service, &r.account, &secret);
            secret.fill(0);
            result?;
            Ok(json!({"configured":true}))
        }
        _ => {
            let mut secret = passwords::get_generic_password(&r.service, &r.account)?;
            let value = if r.operation == "read" {
                let text = std::str::from_utf8(&secret)
                    .map_err(|_| security_framework::base::Error::from_code(-50))?;
                json!({"secret":text})
            } else {
                json!({"authorized":true})
            };
            secret.fill(0);
            Ok(value)
        }
    }
}
fn main() {
    let result = run();
    let mut output = std::io::stdout().lock();
    let _ = serde_json::to_writer(&mut output, &result);
    let _ = output.write_all(b"\n");
}
fn run() -> Value {
    #[cfg(not(target_os = "macos"))]
    return json!({"ok":false,"code":"unsupported","message":"macOS helper only"});
    #[cfg(target_os = "macos")]
    {
        if !authorized_parent() {
            return json!({"ok":false,"code":"credential_helper_caller","message":"Expected the matching signed DevDesk Host"});
        }
        let mut input = Vec::new();
        if std::io::stdin().take(8193).read_to_end(&mut input).is_err() || input.len() > 8192 {
            return json!({"ok":false,"code":"invalid_params"});
        }
        let request = serde_json::from_slice::<Request>(&input);
        input.fill(0);
        let Ok(mut request) = request else {
            return json!({"ok":false,"code":"invalid_params"});
        };
        if !validate(&request) {
            return json!({"ok":false,"code":"invalid_params"});
        }
        match execute(&mut request) {
            Ok(value) => json!({"ok":true,"result":value}),
            Err(error) => {
                json!({"ok":false,"code":match error.code(){-25308|-25293|-128=>"authentication_required",-25300=>"credentials_required",_=>"credential_store"},"message":"System vault denied access; use the explicit settings authorization action"})
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_provider_namespaces_preserve_legacy_accounts() {
        for prefix in ["deepseek-api-key-","dsh-zai-coding-cn-api-key-","dsh-minimax-cn-api-key-"] {
            assert!(valid_account(&format!("{prefix}01234567-89ab-cdef-0123-456789abcdef")));
            assert!(!valid_account(&format!("{prefix}not-an-id")));
        }
        assert!(!valid_account("dsh-ollama-api-key-01234567-89ab-cdef-0123-456789abcdef"));
        assert!(!valid_account("arbitrary-key-01234567-89ab-cdef-0123-456789abcdef"));
    }
    #[test]
    fn rejects_foreign_services_invalid_accounts_and_extra_secret_inputs() {
        let mut request = Request {
            operation: "read".into(),
            service: format!("devdesk.{}", "a".repeat(64)),
            account: "deepseek-official-api-key".into(),
            secret: None,
        };
        assert!(validate(&request));
        request.service = "other-app".into();
        assert!(!validate(&request));
        request.service = format!("devdesk.{}", "a".repeat(64));
        request.account = "../another-secret".into();
        assert!(!validate(&request));
        request.account = "deepseek-official-api-key".into();
        request.secret = Some("not-accepted-for-read".into());
        assert!(!validate(&request));
    }
    #[test]
    #[cfg(target_os = "macos")]
    fn background_native_access_disables_dialogs() {
        use security_framework::os::macos::keychain::SecKeychain;
        let previous = SecKeychain::user_interaction_allowed().unwrap();
        let guard = SecKeychain::disable_user_interaction().unwrap();
        assert!(!SecKeychain::user_interaction_allowed().unwrap());
        drop(guard);
        assert_eq!(previous, SecKeychain::user_interaction_allowed().unwrap());
    }
}
