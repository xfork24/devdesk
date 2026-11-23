//! macOS vault operations run in the signed Host. Only explicit authorization may show UI.
use std::sync::{Mutex, OnceLock, RwLock};

use security_framework::{
    base::Error as KeychainError,
    item::{ItemClass, ItemSearchOptions},
    os::macos::{
        code_signing::{Flags, SecCode, SecRequirement},
        keychain::SecKeychain,
    },
    passwords,
};

use super::*;

const ITEM_NOT_FOUND: i32 = -25300;
static ACCESS: RwLock<()> = RwLock::new(());
static SILENT_ACCESS: Mutex<()> = Mutex::new(());

pub(super) fn available() -> bool {
    static SIGNED_HOST: OnceLock<bool> = OnceLock::new();
    *SIGNED_HOST.get_or_init(|| {
        let Ok(requirement) =
            r#"identifier "dev.devdesk.host" and certificate leaf[subject.CN] exists"#
                .parse::<SecRequirement>()
        else {
            return false;
        };
        SecCode::for_self(Flags::empty())
            .is_ok_and(|code| code.check_validity(Flags::empty(), &requirement).is_ok())
    })
}

fn validate(service: &str, account: &str) -> Result<()> {
    if !available() {
        return Err(err(
            "credential_store_unavailable",
            "钥匙串需要以证书签名的 DevDesk Host，请使用签名后的发行版",
        ));
    }
    let valid_service = service.strip_prefix("devdesk.").is_some_and(|suffix| {
        suffix.len() == 64
            && suffix
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    });
    let valid_account = [
        "deepseek-api-key-",
        "dsh-zai-coding-cn-api-key-",
        "dsh-minimax-cn-api-key-",
        "integration-key-",
    ]
    .iter()
    .any(|prefix| {
        account
            .strip_prefix(prefix)
            .is_some_and(|suffix| suffix.len() == 36 && uuid::Uuid::parse_str(suffix).is_ok())
    });
    require(
        valid_service && valid_account,
        "凭据不属于 DevDesk 钥匙串账户",
    )
}

fn keychain_error(error: KeychainError) -> Error {
    match error.code() {
        -25308 | -25293 | -128 => err(
            "authentication_required",
            "钥匙串尚未授权此 Host，请解锁钥匙串并显式授权",
        ),
        ITEM_NOT_FOUND => err("credentials_required", "钥匙串中没有此凭据"),
        code => err("credential_store", format!("钥匙串操作失败（{code}）")),
    }
}

fn silent<T>(operation: impl FnOnce() -> std::result::Result<T, KeychainError>) -> Result<T> {
    // The Keychain UI switch is process-wide. Keep silent calls serialized and
    // exclude them from the entire explicit authorization, including its dialog.
    let _access = ACCESS.try_read().map_err(|_| {
        err(
            "authentication_required",
            "钥匙串正在等待显式授权，请完成后重试",
        )
    })?;
    let _serial = SILENT_ACCESS.lock().map_err(storage)?;
    let _no_dialog = SecKeychain::disable_user_interaction().map_err(keychain_error)?;
    operation().map_err(keychain_error)
}

pub(super) fn configured(service: &str, account: &str) -> Result<bool> {
    validate(service, account)?;
    silent(|| {
        match ItemSearchOptions::new()
            .class(ItemClass::generic_password())
            .service(service)
            .account(account)
            .load_attributes(true)
            .search()
        {
            Ok(items) => Ok(!items.is_empty()),
            Err(error) if error.code() == ITEM_NOT_FOUND => Ok(false),
            Err(error) => Err(error),
        }
    })
}

pub(super) fn store(service: &str, account: &str, secret: &str) -> Result<()> {
    validate(service, account)?;
    silent(|| passwords::set_generic_password(service, account, secret.as_bytes()))
}

pub(super) fn lookup(service: &str, account: &str) -> Result<Option<Vec<u8>>> {
    validate(service, account)?;
    silent(|| match passwords::get_generic_password(service, account) {
        Ok(secret) => Ok(Some(secret)),
        Err(error) if error.code() == ITEM_NOT_FOUND => Ok(None),
        Err(error) => Err(error),
    })
}

pub(super) fn read(service: &str, account: &str) -> Result<Vec<u8>> {
    lookup(service, account)?.ok_or_else(|| err("credentials_required", "钥匙串中没有此凭据"))
}

pub(super) fn authorize(service: &str, account: &str) -> Result<()> {
    validate(service, account)?;
    let _exclusive = ACCESS.try_write().map_err(|_| {
        err(
            "authentication_required",
            "钥匙串操作正在进行，请稍后重试授权",
        )
    })?;
    let mut secret = passwords::get_generic_password(service, account).map_err(keychain_error)?;
    secret.fill(0);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silent_access_disables_ui_and_excludes_interactive_calls() {
        assert!(SecKeychain::user_interaction_allowed().unwrap());
        silent(|| {
            assert!(!SecKeychain::user_interaction_allowed()?);
            assert!(ACCESS.try_write().is_err());
            Ok(())
        })
        .unwrap();
        assert!(SecKeychain::user_interaction_allowed().unwrap());

        let _exclusive = ACCESS.try_write().unwrap();
        let error = silent(|| -> std::result::Result<(), KeychainError> {
            panic!("background operation entered during interactive authorization");
        })
        .unwrap_err();
        assert_eq!(error.code, "authentication_required");
    }
}
