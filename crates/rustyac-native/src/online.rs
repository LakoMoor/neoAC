// SPDX-License-Identifier: GPL-3.0-or-later

//! `--join host:port`: a server of Kunos' acServer on the local network (or anywhere), joined as
//! `rustyac_game::join::join` joins on Windows: the connection (TCP and UDP), the handshake,
//! the car list, the checksums. The player's id is rustyAC's own (Steam is not asked here), kept
//! in `rustyac_net.ini` in rustyAC's own folder.

use std::hash::{BuildHasher, Hasher};
use std::path::Path;

use rustyac_game::online::{finish_join, BoxedTransport, OnlineSession};
use rustyac_net::client::{Join, JoinError, JOIN_TIMEOUT_MS};
use rustyac_net::handshake::{JoinRequest, Refusal};
use rustyac_net::native::NativeTransport;

pub struct Target {
    pub host: String,
    pub port: u16,
    pub name: String,
    pub team: String,
    pub password: String,
    /// The server's car model (Kunos' acServer gives its pick-up slots by the model).
    pub car: String,
}

/// `host:port`.
pub fn parse_address(text: &str) -> Result<(String, u16), String> {
    let (host, port) = text.rsplit_once(':').ok_or(format!("--join {text}: <host>:<port> is expected (the server's TCP port, e.g. 192.168.1.20:9600)"))?;
    let port = port.parse().map_err(|_| format!("--join {text}: {port:?} is no port number"))?;
    if host.is_empty() {
        return Err(format!("--join {text}: the host is missing"));
    }
    Ok((host.to_string(), port))
}

/// rustyAC's own player id: 8 and sixteen digits, made once.
fn own_guid(file: &Path) -> Result<String, String> {
    let ok = |t: &str| t.len() == 17 && t.starts_with('8') && t.bytes().all(|b| b.is_ascii_digit());
    if file.exists() {
        let text = std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
        return text.lines().find_map(|l| l.trim().strip_prefix("GUID=").map(str::trim).filter(|v| ok(v)).map(str::to_string)).ok_or(format!("{}: no GUID= line with a player id (8 and sixteen digits); delete the file to have a new one made", file.display()));
    }
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos()));
    hasher.write_u32(std::process::id());
    let guid = format!("8{:016}", hasher.finish() % 10_000_000_000_000_000);
    let text = format!("; rustyAC's own player id for online servers, made once from random bits.\n[IDENTITY]\nGUID={guid}\n");
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    std::fs::write(file, text).map_err(|e| format!("{}: {e}", file.display()))?;
    Ok(guid)
}

pub fn default_name() -> String {
    std::env::var("USER").or_else(|_| std::env::var("USERNAME")).unwrap_or_else(|_| "driver".to_string())
}

/// Connects, shakes hands, asks for the cars and prepares the checksums.
pub fn join(target: &Target, own_folder: &Path, root: &Path) -> Result<OnlineSession, String> {
    let guid = own_guid(&own_folder.join("rustyac_net.ini"))?;
    let request = JoinRequest { guid, name: target.name.clone(), team: target.team.clone(), nation_code: String::new(), car: target.car.clone(), password: target.password.clone() };
    println!("online: joining {}:{} as {:?}", target.host, target.port, request.name);
    let transport: BoxedTransport = Box::new(NativeTransport::connect(&target.host, target.port, 8000)?);
    let mut join = Join::start(transport, &request).map_err(|e| e.to_string())?;
    join.wait(JOIN_TIMEOUT_MS).map_err(|e| {
        let hint = if request.car.is_empty() && matches!(e, JoinError::Refused(Refusal::NoSlots)) { " (no car was asked for: name one of the server's models with --car)" } else { "" };
        format!("{e}{hint}")
    })?;
    let (client, cars) = join.finish().ok_or("the join did not end")?;
    let a = client.accepted.clone();
    println!("online: {:?} lets you in: slot {}, {} ({}) at {}{}, session {:?}, {} updates a second", a.server_name, a.session_id, a.model, a.skin, a.track, if a.track_config.is_empty() { String::new() } else { format!(" ({})", a.track_config) }, a.current_session.name, a.send_hz);
    let session = finish_join(client, cars.clone(), Some(root))?;
    let others = session.remote_entries.iter().filter(|car| !car.driver_name.is_empty()).count();
    println!("online: {} cars on the server, {} other driver(s) there now", cars.len(), others);
    Ok(session)
}
