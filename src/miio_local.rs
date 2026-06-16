use aes::Aes128;
use anyhow::{anyhow, bail, Result};
use cbc::cipher::{block_padding::Pkcs7, BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use serde_json::{json, Value};
use std::net::UdpSocket;
use std::time::Duration;

type Aes128CbcEnc = cbc::Encryptor<Aes128>;
type Aes128CbcDec = cbc::Decryptor<Aes128>;

const MIIO_PORT: u16 = 54321;
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Clone, Debug)]
pub struct MiioUdpClient {
    addr: String,
    token: [u8; 16],
    timeout: Duration,
}

impl MiioUdpClient {
    pub fn new(addr: &str, token_hex: &str) -> Result<Self> {
        Self::with_timeout(addr, token_hex, DEFAULT_TIMEOUT)
    }

    pub fn with_timeout(addr: &str, token_hex: &str, timeout: Duration) -> Result<Self> {
        Ok(Self {
            addr: normalize_udp_addr(addr),
            token: parse_token_hex(token_hex)?,
            timeout,
        })
    }

    pub fn probe(&self) -> Result<()> {
        let socket = UdpSocket::bind("0.0.0.0:0")?;
        socket.set_read_timeout(Some(self.timeout))?;
        socket.set_write_timeout(Some(self.timeout))?;
        socket.connect(self.addr.as_str())?;
        let _ = handshake(&socket)?;
        Ok(())
    }

    pub fn request(&self, method: &str, params: Value) -> Result<Value> {
        let socket = UdpSocket::bind("0.0.0.0:0")?;
        socket.set_read_timeout(Some(self.timeout))?;
        socket.set_write_timeout(Some(self.timeout))?;
        socket.connect(self.addr.as_str())?;

        let (device_id, stamp) = handshake(&socket)?;
        let payload = json!({
            "id": 1,
            "method": method,
            "params": params,
        });
        let payload_text = serde_json::to_string(&payload)?;
        let packet = build_command_packet(
            device_id,
            stamp.saturating_add(1),
            &self.token,
            payload_text.as_bytes(),
        )?;
        socket.send(&packet)?;

        let (response_header, response_payload) = recv_packet(&socket)?;
        if response_header.packet_length < 32 {
            bail!("miio 响应头长度无效");
        }
        let decrypted = decrypt_payload(&self.token, &response_payload)?;
        // Some devices append a redundant trailing NUL after the JSON.
        let mut end = decrypted.len();
        while end > 0 && decrypted[end - 1] == 0 {
            end -= 1;
        }
        let value: Value = serde_json::from_slice(&decrypted[..end])?;
        if value.get("error").is_some() {
            bail!("miio 返回错误响应: {}", value);
        }
        Ok(value.get("result").cloned().unwrap_or(value))
    }
}

#[derive(Clone, Copy, Debug)]
struct HeaderParts {
    packet_length: u16,
    device_id: u32,
    stamp: u32,
}

fn handshake(socket: &UdpSocket) -> Result<(u32, u32)> {
    let packet = build_handshake_packet();
    socket.send(&packet)?;
    let (header, _) = recv_packet(socket)?;
    Ok((header.device_id, header.stamp))
}

fn recv_packet(socket: &UdpSocket) -> Result<(HeaderParts, Vec<u8>)> {
    let mut buf = [0_u8; 65535];
    let size = socket.recv(&mut buf)?;
    if size < 32 {
        bail!("miio 响应过短");
    }
    if buf[0] != 0x21 || buf[1] != 0x31 {
        bail!("miio 魔术头无效");
    }
    let packet_length = u16::from_be_bytes([buf[2], buf[3]]);
    if packet_length as usize > size {
        bail!("miio 响应被截断");
    }
    let payload = if packet_length as usize > 32 {
        buf[32..packet_length as usize].to_vec()
    } else {
        Vec::new()
    };
    Ok((
        HeaderParts {
            packet_length,
            device_id: u32::from_be_bytes([buf[8], buf[9], buf[10], buf[11]]),
            stamp: u32::from_be_bytes([buf[12], buf[13], buf[14], buf[15]]),
        },
        payload,
    ))
}

pub fn build_handshake_packet() -> Vec<u8> {
    let mut out = Vec::with_capacity(32);
    out.extend_from_slice(&[0x21, 0x31]); // magic
    out.extend_from_slice(&32_u16.to_be_bytes()); // length
    out.extend_from_slice(&0xffff_ffff_u32.to_be_bytes()); // unknown
    out.extend_from_slice(&0xffff_ffff_u32.to_be_bytes()); // device id
    out.extend_from_slice(&0xffff_ffff_u32.to_be_bytes()); // stamp
    out.extend_from_slice(&[0xff; 16]); // checksum
    out
}

fn build_command_packet(
    device_id: u32,
    stamp: u32,
    token: &[u8; 16],
    payload: &[u8],
) -> Result<Vec<u8>> {
    let encrypted = encrypt_payload(token, payload)?;
    let packet_len = (32 + encrypted.len()) as u16;
    let mut packet = Vec::with_capacity(packet_len as usize);
    packet.extend_from_slice(&[0x21, 0x31]);
    packet.extend_from_slice(&packet_len.to_be_bytes());
    packet.extend_from_slice(&0_u32.to_be_bytes());
    packet.extend_from_slice(&device_id.to_be_bytes());
    packet.extend_from_slice(&stamp.to_be_bytes());
    packet.extend_from_slice(token);
    packet.extend_from_slice(&encrypted);

    let checksum = md5::compute(&packet);
    packet[16..32].copy_from_slice(&checksum.0);
    Ok(packet)
}

pub fn parse_token_hex(raw: &str) -> Result<[u8; 16]> {
    let hex = raw.trim();
    if hex.len() != 32 {
        bail!("miio token 必须是 32 位十六进制字符串");
    }
    let mut out = [0_u8; 16];
    for (index, slot) in out.iter_mut().enumerate() {
        let start = index * 2;
        *slot = u8::from_str_radix(&hex[start..start + 2], 16)
            .map_err(|_| anyhow!("miio token 不是合法十六进制"))?;
    }
    Ok(out)
}

pub fn encrypt_payload(token: &[u8; 16], payload: &[u8]) -> Result<Vec<u8>> {
    let key = md5::compute(token).0;
    let mut iv_src = Vec::with_capacity(32);
    iv_src.extend_from_slice(&key);
    iv_src.extend_from_slice(token);
    let iv = md5::compute(iv_src).0;
    let padded_len = (payload.len() / 16 + 1) * 16;
    let mut buf = vec![0u8; padded_len];
    buf[..payload.len()].copy_from_slice(payload);
    let encrypted = Aes128CbcEnc::new_from_slices(&key, &iv)
        .map_err(|e| anyhow!("miio AES 初始化失败: {e}"))?
        .encrypt_padded_mut::<Pkcs7>(&mut buf, payload.len())
        .map_err(|e| anyhow!("miio AES 加密失败: {e:?}"))?;
    Ok(encrypted.to_vec())
}

pub fn decrypt_payload(token: &[u8; 16], payload: &[u8]) -> Result<Vec<u8>> {
    let key = md5::compute(token).0;
    let mut iv_src = Vec::with_capacity(32);
    iv_src.extend_from_slice(&key);
    iv_src.extend_from_slice(token);
    let iv = md5::compute(iv_src).0;
    let mut buf = payload.to_vec();
    let decrypted = Aes128CbcDec::new_from_slices(&key, &iv)
        .map_err(|e| anyhow!("miio AES 初始化失败: {e}"))?
        .decrypt_padded_mut::<Pkcs7>(&mut buf)
        .map_err(|e| anyhow!("miio AES 解密失败: {e:?}"))?;
    Ok(decrypted.to_vec())
}

fn normalize_udp_addr(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.contains(':') {
        trimmed.to_string()
    } else {
        format!("{trimmed}:{MIIO_PORT}")
    }
}
