use super::*;

pub(super) const WIRE_VERSION: u32 = 1;
pub(super) const MAX_FRAME_BYTES: usize = 4 * 1024 * 1024;
pub(super) const DESCRIBE: &str = "$describe";
pub(super) const PROVIDER: &str = "$provider";

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct WireRequest {
    pub(super) version: u32,
    pub(super) capability: CapabilityRef,
    pub(super) operation: String,
    pub(super) arguments: Value,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct WireResponse {
    pub(super) version: u32,
    pub(super) result: Result<Value, RpcError>,
}

pub(super) fn configure_timeouts(stream: &TcpStream) -> Result<(), RpcError> {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|error| RpcError::Transport(error.to_string()))?;
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .map_err(|error| RpcError::Transport(error.to_string()))
}

pub(super) fn write_json_line<T: Serialize>(
    stream: &mut TcpStream,
    value: &T,
) -> Result<(), RpcError> {
    let mut data = serde_json::to_vec(value).map_err(|error| RpcError::Codec(error.to_string()))?;
    if data.len() + 1 > MAX_FRAME_BYTES {
        return Err(RpcError::Transport(
            "RPC request or response too large".into(),
        ));
    }
    data.push(b'\n');
    stream
        .write_all(&data)
        .map_err(|error| RpcError::Transport(error.to_string()))
}

pub(super) fn read_json_line<T: DeserializeOwned>(stream: &mut TcpStream) -> Result<T, RpcError> {
    read_json_line_optional(stream)?
        .ok_or_else(|| RpcError::Transport("RPC connection closed before complete frame".into()))
}

pub(super) fn read_json_line_optional<T: DeserializeOwned>(
    stream: &mut TcpStream,
) -> Result<Option<T>, RpcError> {
    let mut reader = BufReader::new(stream);
    let mut bytes = Vec::new();
    loop {
        let buffer = reader
            .fill_buf()
            .map_err(|error| RpcError::Transport(error.to_string()))?;
        if buffer.is_empty() {
            return if bytes.is_empty() {
                Ok(None)
            } else {
                Err(RpcError::Transport(
                    "RPC connection closed before complete frame".into(),
                ))
            };
        }
        let newline = buffer.iter().position(|byte| *byte == b'\n');
        let length = newline.map(|index| index + 1).unwrap_or(buffer.len());
        if bytes.len() + length > MAX_FRAME_BYTES {
            return Err(RpcError::Transport("RPC frame is too large".into()));
        }
        bytes.extend_from_slice(&buffer[..length]);
        reader.consume(length);
        if newline.is_some() {
            break;
        }
    }
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|error| RpcError::Codec(error.to_string()))
}
