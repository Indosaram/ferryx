//! Linux: the vault key as an item in the Secret Service default collection
//! (org.freedesktop.secrets: GNOME Keyring, KeePassXC, KWallet's provider), over the session
//! bus with the blocking zbus API. A session without the service reports `Unavailable`.

use std::collections::HashMap;

use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

use super::{KeyStore, KeystoreError, KEY_SERVICE};

const DEST: &str = "org.freedesktop.secrets";
const SERVICE_PATH: &str = "/org/freedesktop/secrets";
const DEFAULT_COLLECTION: &str = "/org/freedesktop/secrets/aliases/default";
const SERVICE_IFACE: &str = "org.freedesktop.Secret.Service";
const COLLECTION_IFACE: &str = "org.freedesktop.Secret.Collection";
const ITEM_IFACE: &str = "org.freedesktop.Secret.Item";
const PROMPT_IFACE: &str = "org.freedesktop.Secret.Prompt";
/// The "no prompt needed" object path.
const NO_PROMPT: &str = "/";

/// Secret Service `Secret` struct: (session, parameters, value, content type).
type Secret = (OwnedObjectPath, Vec<u8>, Vec<u8>, String);

pub(super) struct SecretService;

fn unavailable(error: zbus::Error) -> KeystoreError {
    KeystoreError::Unavailable(format!(
        "no Secret Service on the session bus (install and unlock GNOME Keyring or KeePassXC): {error}"
    ))
}

fn failed(error: zbus::Error) -> KeystoreError {
    KeystoreError::Failed(format!("Secret Service: {error}"))
}

struct Session {
    connection: Connection,
    path: OwnedObjectPath,
}

impl Session {
    fn open() -> Result<Self, KeystoreError> {
        let connection = Connection::session().map_err(unavailable)?;
        let service = Proxy::new(&connection, DEST, SERVICE_PATH, SERVICE_IFACE).map_err(unavailable)?;
        // "plain": the secret travels over the local session bus only.
        let (_output, path): (OwnedValue, OwnedObjectPath) =
            service.call("OpenSession", &("plain", Value::from(""))).map_err(unavailable)?;
        Ok(Self { connection, path })
    }

    fn proxy<'a>(&'a self, path: &'a str, iface: &'a str) -> Result<Proxy<'a>, KeystoreError> {
        Proxy::new(&self.connection, DEST, path, iface).map_err(failed)
    }

    fn attributes(account: &str) -> HashMap<&'static str, String> {
        HashMap::from([("service", KEY_SERVICE.to_string()), ("account", account.to_string())])
    }

    /// Items for `account`, unlocking locked ones (which may show the keyring's own prompt).
    fn find(&self, account: &str) -> Result<Vec<OwnedObjectPath>, KeystoreError> {
        let service = self.proxy(SERVICE_PATH, SERVICE_IFACE)?;
        let (mut unlocked, locked): (Vec<OwnedObjectPath>, Vec<OwnedObjectPath>) =
            service.call("SearchItems", &(Self::attributes(account),)).map_err(failed)?;
        if !locked.is_empty() {
            let (now_unlocked, prompt): (Vec<OwnedObjectPath>, OwnedObjectPath) =
                service.call("Unlock", &(locked,)).map_err(failed)?;
            unlocked.extend(now_unlocked);
            if prompt.as_str() != NO_PROMPT {
                self.prompt(&prompt)?;
                return self.find_unlocked(account);
            }
        }
        Ok(unlocked)
    }

    fn find_unlocked(&self, account: &str) -> Result<Vec<OwnedObjectPath>, KeystoreError> {
        let service = self.proxy(SERVICE_PATH, SERVICE_IFACE)?;
        let (unlocked, _locked): (Vec<OwnedObjectPath>, Vec<OwnedObjectPath>) =
            service.call("SearchItems", &(Self::attributes(account),)).map_err(failed)?;
        Ok(unlocked)
    }

    /// Runs a Secret Service prompt and waits for its Completed signal.
    fn prompt(&self, path: &OwnedObjectPath) -> Result<(), KeystoreError> {
        let prompt = self.proxy(path.as_str(), PROMPT_IFACE)?;
        // Subscribe before triggering, so the signal cannot be missed.
        let mut completed = prompt.receive_signal("Completed").map_err(failed)?;
        prompt.call::<_, _, ()>("Prompt", &("",)).map_err(failed)?;
        let message = completed
            .next()
            .ok_or_else(|| KeystoreError::Failed("the Secret Service prompt ended without an answer".into()))?;
        let (dismissed, _result): (bool, OwnedValue) = message.body().deserialize().map_err(failed)?;
        if dismissed {
            return Err(KeystoreError::Failed("the keyring unlock prompt was dismissed".into()));
        }
        Ok(())
    }
}

impl KeyStore for SecretService {
    fn backend(&self) -> &'static str {
        "secretService"
    }

    fn load(&self, account: &str) -> Result<Option<Vec<u8>>, KeystoreError> {
        let session = Session::open()?;
        let Some(item) = session.find(account)?.into_iter().next() else {
            return Ok(None);
        };
        let item = session.proxy(item.as_str(), ITEM_IFACE)?;
        let (_session, _parameters, value, _content_type): Secret =
            item.call("GetSecret", &(session.path.clone(),)).map_err(failed)?;
        Ok(Some(value))
    }

    fn store(&self, account: &str, secret: &[u8]) -> Result<(), KeystoreError> {
        let session = Session::open()?;
        let collection = session.proxy(DEFAULT_COLLECTION, COLLECTION_IFACE)?;
        let attributes: HashMap<String, String> = Session::attributes(account)
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect();
        let properties: HashMap<&str, Value<'_>> = HashMap::from([
            ("org.freedesktop.Secret.Item.Label", Value::from("Ferryx browser vault key")),
            ("org.freedesktop.Secret.Item.Attributes", Value::from(attributes)),
        ]);
        let value: Secret = (
            session.path.clone(),
            Vec::new(),
            secret.to_vec(),
            "application/octet-stream".to_string(),
        );
        let (_item, prompt): (OwnedObjectPath, OwnedObjectPath) =
            collection.call("CreateItem", &(properties, value, true)).map_err(failed)?;
        if prompt.as_str() != NO_PROMPT {
            session.prompt(&prompt)?;
        }
        Ok(())
    }

    fn delete(&self, account: &str) -> Result<(), KeystoreError> {
        let session = Session::open()?;
        for item in session.find(account)? {
            let item = session.proxy(item.as_str(), ITEM_IFACE)?;
            let prompt: OwnedObjectPath = item.call("Delete", &()).map_err(failed)?;
            if prompt.as_str() != NO_PROMPT {
                session.prompt(&prompt)?;
            }
        }
        Ok(())
    }
}
