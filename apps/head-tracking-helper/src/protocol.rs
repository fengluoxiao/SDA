use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u8 = 1;
pub const MAX_COMMAND_BYTES: usize = 4096;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Command {
    #[serde(rename = "type")]
    pub kind: CommandType,
    pub protocol: u8,
    pub session: String,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CommandType {
    Start,
    Stop,
    Recenter,
}

impl Command {
    pub fn parse(line: &str) -> Result<Self, String> {
        if line.len() > MAX_COMMAND_BYTES {
            return Err("command exceeds 4 KiB".into());
        }
        let command: Self = serde_json::from_str(line).map_err(|_| "invalid command JSON")?;
        if command.protocol != PROTOCOL_VERSION {
            return Err("unsupported protocol version".into());
        }
        if !valid_session(&command.session) {
            return Err("invalid session token".into());
        }
        Ok(command)
    }

    pub fn validate_session(&self, session: &str) -> Result<(), String> {
        if self.protocol != PROTOCOL_VERSION || self.session != session {
            return Err("protocol or session mismatch".into());
        }
        Ok(())
    }
}

fn valid_session(value: &str) -> bool {
    value.len() == 64
        && value
            .as_bytes()
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
}

#[derive(Serialize)]
pub struct Hello<'a> {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub protocol: u8,
    pub session: &'a str,
    pub source: &'static str,
    #[serde(rename = "coordinateSystem")]
    pub coordinate_system: &'static str,
    pub orientation: &'static str,
}

#[derive(Serialize)]
pub struct Status<'a> {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub protocol: u8,
    pub session: &'a str,
    pub state: &'static str,
    pub detail: &'a str,
}

#[derive(Serialize)]
pub struct ErrorMessage<'a> {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub protocol: u8,
    pub session: &'a str,
    pub code: &'static str,
    pub message: &'a str,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Orientation {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub w: f64,
}

#[derive(Serialize)]
pub struct Pose<'a> {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub protocol: u8,
    pub session: &'a str,
    pub seq: u64,
    #[serde(rename = "timestampMs")]
    pub timestamp_ms: u128,
    pub orientation: Orientation,
}

#[cfg(test)]
mod tests {
    use super::*;

    const SESSION: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn accepts_exact_start_contract() {
        let command = Command::parse(&format!(
            r#"{{"type":"start","protocol":1,"session":"{SESSION}"}}"#
        ))
        .unwrap();
        assert_eq!(command.kind, CommandType::Start);
    }

    #[test]
    fn rejects_extra_fields_and_bad_tokens() {
        assert!(
            Command::parse(&format!(
                r#"{{"type":"start","protocol":1,"session":"{SESSION}","address":"secret"}}"#
            ))
            .is_err()
        );
        assert!(Command::parse(r#"{"type":"start","protocol":1,"session":"ABC"}"#).is_err());
    }
}
