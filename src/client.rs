//! The window's and the command line's side of the service's D-Bus interface.

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use crate::daemon::{BUS_NAME, INTERFACE, OBJECT_PATH, name_has_owner, spawn_detached};
use crate::model::{Request, Response};

fn connection() -> Result<&'static zbus::blocking::Connection, String> {
    static CONNECTION: OnceLock<Result<zbus::blocking::Connection, String>> = OnceLock::new();
    CONNECTION
        .get_or_init(|| zbus::blocking::Connection::session().map_err(|e| e.to_string()))
        .as_ref()
        .map_err(Clone::clone)
}

pub fn request(request: &Request) -> Result<Response, String> {
    let body = serde_json::to_string(request).map_err(|e| e.to_string())?;
    let reply = connection()?
        .call_method(
            Some(BUS_NAME),
            OBJECT_PATH,
            Some(INTERFACE),
            "Request",
            &(body,),
        )
        .map_err(|e| match e {
            zbus::Error::MethodError(name, _, _) if name.as_str().ends_with("ServiceUnknown") => {
                "The Tessera service is not running.".to_string()
            }
            other => other.to_string(),
        })?;
    let text: String = reply.body().deserialize().map_err(|e| e.to_string())?;
    let response: Response = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    match response {
        Response::Error { message } => Err(message),
        other => Ok(other),
    }
}

pub fn revision() -> Option<u64> {
    connection()
        .ok()?
        .call_method(
            Some(BUS_NAME),
            OBJECT_PATH,
            Some(INTERFACE),
            "Revision",
            &(),
        )
        .ok()?
        .body()
        .deserialize()
        .ok()
}

pub fn service_running() -> bool {
    name_has_owner(BUS_NAME)
}

/// Start the service if it is not running and wait until it answers.
pub fn ensure_service() -> bool {
    if service_running() {
        return true;
    }
    spawn_detached(&["--daemon"]);
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(100));
        if service_running() {
            return true;
        }
    }
    false
}

/// Ask the service to quit and wait until it has.
pub fn stop_service() {
    if request(&Request::Quit).is_err() {
        return;
    }
    let start = Instant::now();
    while service_running() && start.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(100));
    }
}
