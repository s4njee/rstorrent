//! Minimal XML-RPC codec tailored to rtorrent.
//!
//! We hand-roll this instead of using a generic XML-RPC crate for two reasons:
//!   1. rtorrent speaks XML-RPC over SCGI, which no general crate implements.
//!   2. rtorrent emits the non-standard `<i8>` tag (a 64-bit int extension from
//!      xmlrpc-c). Many parsers choke on it; here we treat `<i4>`, `<int>`, and
//!      `<i8>` identically.
//!
//! Only the value kinds rtorrent actually uses are supported: int, boolean,
//! string, double, base64, array, struct, and `<fault>`. The encoder always
//! sends integers as `<i8>` so large byte counts round-trip safely.

use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use quick_xml::events::Event;
use quick_xml::Reader;

use super::{Result, RtorrentError};

/// A dynamically-typed XML-RPC value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i64),
    Bool(bool),
    Str(String),
    Double(f64),
    Bytes(Vec<u8>),
    Array(Vec<Value>),
    /// Ordered key/value members (XML-RPC structs preserve author order).
    Struct(Vec<(String, Value)>),
}

impl Value {
    /// Integer view; booleans read as 0/1 so callers can treat rtorrent's
    /// boolean-ish fields uniformly.
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Int(n) => Some(*n),
            Value::Bool(b) => Some(*b as i64),
            _ => None,
        }
    }

    /// Boolean view: explicit bool, or any non-zero integer.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            Value::Int(n) => Some(*n != 0),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }

    /// Look up a struct member by key.
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Struct(members) => members.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Encoding
// ---------------------------------------------------------------------------

/// Serialize a `methodCall` document for the given method and parameters.
pub fn method_call(name: &str, params: &[Value]) -> String {
    let mut s = String::with_capacity(128 + params.len() * 32);
    s.push_str("<?xml version=\"1.0\"?><methodCall><methodName>");
    escape_into(&mut s, name);
    s.push_str("</methodName><params>");
    for p in params {
        s.push_str("<param><value>");
        encode_value(&mut s, p);
        s.push_str("</value></param>");
    }
    s.push_str("</params></methodCall>");
    s
}

/// Write a value's typed body (without the surrounding `<value>` tags).
fn encode_value(out: &mut String, v: &Value) {
    match v {
        // Always widen to i8 so 64-bit byte counts survive the round-trip.
        Value::Int(n) => {
            out.push_str("<i8>");
            out.push_str(&n.to_string());
            out.push_str("</i8>");
        }
        Value::Bool(b) => {
            out.push_str("<boolean>");
            out.push(if *b { '1' } else { '0' });
            out.push_str("</boolean>");
        }
        Value::Str(s) => {
            out.push_str("<string>");
            escape_into(out, s);
            out.push_str("</string>");
        }
        Value::Double(d) => {
            out.push_str("<double>");
            out.push_str(&d.to_string());
            out.push_str("</double>");
        }
        Value::Bytes(b) => {
            out.push_str("<base64>");
            out.push_str(&B64.encode(b));
            out.push_str("</base64>");
        }
        Value::Array(items) => {
            out.push_str("<array><data>");
            for it in items {
                out.push_str("<value>");
                encode_value(out, it);
                out.push_str("</value>");
            }
            out.push_str("</data></array>");
        }
        Value::Struct(members) => {
            out.push_str("<struct>");
            for (k, val) in members {
                out.push_str("<member><name>");
                escape_into(out, k);
                out.push_str("</name><value>");
                encode_value(out, val);
                out.push_str("</value></member>");
            }
            out.push_str("</struct>");
        }
    }
}

/// XML-escape text content (`&`, `<`, `>` are enough for element bodies).
fn escape_into(out: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
}

// ---------------------------------------------------------------------------
// Decoding
// ---------------------------------------------------------------------------

/// Parse a `methodResponse` body, returning its single value or the `<fault>`
/// mapped to [`RtorrentError::Fault`].
pub fn parse_response(xml: &[u8]) -> Result<Value> {
    let mut reader = Reader::from_reader(xml);
    // rtorrent output is compact; trimming keeps structural parsing simple. The
    // only cost is that a string value padded with leading/trailing spaces loses
    // that padding — not a concern for the fields we read.
    reader.config_mut().trim_text(true);

    let mut buf = Vec::new();
    let mut in_fault = false;

    loop {
        match reader
            .read_event_into(&mut buf)
            .map_err(|e| RtorrentError::Parse(e.to_string()))?
        {
            Event::Start(e) => match e.name().as_ref() {
                b"fault" => in_fault = true,
                b"value" => {
                    let v = read_value(&mut reader, &mut buf)?;
                    return if in_fault { Err(fault_from(v)) } else { Ok(v) };
                }
                // methodResponse / params / param — descend into them.
                _ => {}
            },
            Event::Eof => {
                return Err(RtorrentError::Parse("no <value> in response".into()));
            }
            _ => {}
        }
        buf.clear();
    }
}

/// Turn a fault struct (`faultCode` / `faultString`) into a typed error.
fn fault_from(v: Value) -> RtorrentError {
    let code = v.get("faultCode").and_then(Value::as_i64).unwrap_or(-1);
    let message = v
        .get("faultString")
        .and_then(Value::as_str)
        .unwrap_or("unknown fault")
        .to_string();
    RtorrentError::Fault { code, message }
}

/// Read a value. Precondition: the opening `<value>` was just consumed; this
/// reads through and consumes the matching `</value>`.
fn read_value(reader: &mut Reader<&[u8]>, buf: &mut Vec<u8>) -> Result<Value> {
    loop {
        let ev = reader
            .read_event_into(buf)
            .map_err(|e| RtorrentError::Parse(e.to_string()))?;
        match ev {
            Event::Start(e) => {
                // Copy the tag name out so `e` (which borrows `buf`) is dropped
                // before we pass `buf` back into the readers below.
                let tag = e.name().as_ref().to_vec();
                let value = match tag.as_slice() {
                    b"i4" | b"int" | b"i8" => {
                        let t = read_text(reader, buf, &tag)?;
                        Value::Int(
                            t.trim()
                                .parse::<i64>()
                                .map_err(|_| RtorrentError::Parse(format!("bad int '{t}'")))?,
                        )
                    }
                    b"boolean" => {
                        let t = read_text(reader, buf, b"boolean")?;
                        Value::Bool(t.trim() == "1")
                    }
                    b"string" => Value::Str(read_text(reader, buf, b"string")?),
                    b"double" => {
                        let t = read_text(reader, buf, b"double")?;
                        Value::Double(
                            t.trim()
                                .parse::<f64>()
                                .map_err(|_| RtorrentError::Parse(format!("bad double '{t}'")))?,
                        )
                    }
                    b"base64" => {
                        let t = read_text(reader, buf, b"base64")?;
                        Value::Bytes(
                            B64.decode(t.trim())
                                .map_err(|e| RtorrentError::Parse(e.to_string()))?,
                        )
                    }
                    b"array" => read_array(reader, buf)?,
                    b"struct" => read_struct(reader, buf)?,
                    // Unknown/`<nil/>`: skip to its end and treat as empty string.
                    _ => {
                        skip_to_end(reader, buf, &tag)?;
                        Value::Str(String::new())
                    }
                };
                consume_value_end(reader, buf)?;
                return Ok(value);
            }
            // Bare text inside <value> is an untyped string per the spec.
            Event::Text(t) => {
                let s = t
                    .unescape()
                    .map_err(|e| RtorrentError::Parse(e.to_string()))?
                    .into_owned();
                consume_value_end(reader, buf)?;
                return Ok(Value::Str(s));
            }
            // Empty element `<value></value>` → empty string.
            Event::End(e) if e.name().as_ref() == b"value" => return Ok(Value::Str(String::new())),
            Event::Eof => return Err(RtorrentError::Parse("eof inside <value>".into())),
            _ => {}
        }
    }
}

/// Read the text content of a simple element up to and including `</tag>`.
fn read_text(reader: &mut Reader<&[u8]>, buf: &mut Vec<u8>, tag: &[u8]) -> Result<String> {
    let mut out = String::new();
    loop {
        match reader
            .read_event_into(buf)
            .map_err(|e| RtorrentError::Parse(e.to_string()))?
        {
            Event::Text(t) => out.push_str(
                &t.unescape()
                    .map_err(|e| RtorrentError::Parse(e.to_string()))?,
            ),
            Event::CData(t) => out.push_str(
                std::str::from_utf8(&t).map_err(|e| RtorrentError::Parse(e.to_string()))?,
            ),
            Event::End(e) if e.name().as_ref() == tag => return Ok(out),
            Event::Eof => return Err(RtorrentError::Parse("eof reading text".into())),
            _ => {}
        }
    }
}

/// Read an `<array>` body; precondition: `<array>` was just consumed.
fn read_array(reader: &mut Reader<&[u8]>, buf: &mut Vec<u8>) -> Result<Value> {
    let mut items = Vec::new();
    // Advance to <data> (or bail on an empty/degenerate array).
    loop {
        match reader
            .read_event_into(buf)
            .map_err(|e| RtorrentError::Parse(e.to_string()))?
        {
            Event::Start(e) if e.name().as_ref() == b"data" => break,
            Event::End(e) if e.name().as_ref() == b"array" => return Ok(Value::Array(items)),
            Event::Eof => return Err(RtorrentError::Parse("eof in <array>".into())),
            _ => {}
        }
    }
    // Read <value> children until </data>.
    loop {
        match reader
            .read_event_into(buf)
            .map_err(|e| RtorrentError::Parse(e.to_string()))?
        {
            Event::Start(e) if e.name().as_ref() == b"value" => {
                items.push(read_value(reader, buf)?)
            }
            Event::End(e) if e.name().as_ref() == b"data" => break,
            Event::Eof => return Err(RtorrentError::Parse("eof in <data>".into())),
            _ => {}
        }
    }
    skip_to_end(reader, buf, b"array")?;
    Ok(Value::Array(items))
}

/// Read a `<struct>` body; precondition: `<struct>` was just consumed.
fn read_struct(reader: &mut Reader<&[u8]>, buf: &mut Vec<u8>) -> Result<Value> {
    let mut members = Vec::new();
    loop {
        match reader
            .read_event_into(buf)
            .map_err(|e| RtorrentError::Parse(e.to_string()))?
        {
            Event::Start(e) if e.name().as_ref() == b"member" => {
                members.push(read_member(reader, buf)?)
            }
            Event::End(e) if e.name().as_ref() == b"struct" => break,
            Event::Eof => return Err(RtorrentError::Parse("eof in <struct>".into())),
            _ => {}
        }
    }
    Ok(Value::Struct(members))
}

/// Read one `<member>`: a `<name>` and a `<value>`.
fn read_member(reader: &mut Reader<&[u8]>, buf: &mut Vec<u8>) -> Result<(String, Value)> {
    let mut name = String::new();
    let mut value = Value::Str(String::new());
    loop {
        match reader
            .read_event_into(buf)
            .map_err(|e| RtorrentError::Parse(e.to_string()))?
        {
            Event::Start(e) if e.name().as_ref() == b"name" => {
                name = read_text(reader, buf, b"name")?;
            }
            Event::Start(e) if e.name().as_ref() == b"value" => {
                value = read_value(reader, buf)?;
            }
            Event::End(e) if e.name().as_ref() == b"member" => break,
            Event::Eof => return Err(RtorrentError::Parse("eof in <member>".into())),
            _ => {}
        }
    }
    Ok((name, value))
}

/// Consume events up to and including `</value>`.
fn consume_value_end(reader: &mut Reader<&[u8]>, buf: &mut Vec<u8>) -> Result<()> {
    skip_to_end(reader, buf, b"value")
}

/// Skip events until the matching end tag `</name>` is consumed.
fn skip_to_end(reader: &mut Reader<&[u8]>, buf: &mut Vec<u8>, name: &[u8]) -> Result<()> {
    loop {
        match reader
            .read_event_into(buf)
            .map_err(|e| RtorrentError::Parse(e.to_string()))?
        {
            Event::End(e) if e.name().as_ref() == name => return Ok(()),
            Event::Eof => return Err(RtorrentError::Parse("eof skipping to end".into())),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_method_call_with_i8() {
        let xml = method_call("d.start", &[Value::Str("HASH".into())]);
        assert!(xml.contains("<methodName>d.start</methodName>"));
        assert!(xml.contains("<string>HASH</string>"));
    }

    #[test]
    fn encodes_int_as_i8() {
        let xml = method_call("x", &[Value::Int(5_000_000_000)]);
        assert!(xml.contains("<i8>5000000000</i8>"));
    }

    #[test]
    fn parses_i8_scalar() {
        let xml = br#"<?xml version="1.0"?><methodResponse><params><param>
            <value><i8>5000000000</i8></value></param></params></methodResponse>"#;
        assert_eq!(parse_response(xml).unwrap(), Value::Int(5_000_000_000));
    }

    #[test]
    fn parses_string_and_escapes() {
        let xml = br#"<methodResponse><params><param>
            <value><string>a &amp; b</string></value></param></params></methodResponse>"#;
        assert_eq!(parse_response(xml).unwrap(), Value::Str("a & b".into()));
    }

    #[test]
    fn parses_multicall_array_of_rows() {
        // Shape of a d.multicall2 result: array of arrays of values.
        let xml = br#"<methodResponse><params><param><value><array><data>
            <value><array><data>
                <value><string>NAME1</string></value>
                <value><i8>1024</i8></value>
            </data></array></value>
            <value><array><data>
                <value><string>NAME2</string></value>
                <value><i8>2048</i8></value>
            </data></array></value>
        </data></array></value></param></params></methodResponse>"#;
        let v = parse_response(xml).unwrap();
        let rows = v.as_array().unwrap();
        assert_eq!(rows.len(), 2);
        let row0 = rows[0].as_array().unwrap();
        assert_eq!(row0[0], Value::Str("NAME1".into()));
        assert_eq!(row0[1], Value::Int(1024));
    }

    #[test]
    fn parses_fault_as_error() {
        let xml = br#"<methodResponse><fault><value><struct>
            <member><name>faultCode</name><value><i4>-501</i4></value></member>
            <member><name>faultString</name><value><string>boom</string></value></member>
        </struct></value></fault></methodResponse>"#;
        match parse_response(xml) {
            Err(RtorrentError::Fault { code, message }) => {
                assert_eq!(code, -501);
                assert_eq!(message, "boom");
            }
            other => panic!("expected fault, got {other:?}"),
        }
    }

    #[test]
    fn parses_boolean_and_base64() {
        let xml = br#"<methodResponse><params><param><value><struct>
            <member><name>b</name><value><boolean>1</boolean></value></member>
            <member><name>d</name><value><base64>aGk=</base64></value></member>
        </struct></value></param></params></methodResponse>"#;
        let v = parse_response(xml).unwrap();
        assert_eq!(v.get("b").unwrap().as_bool(), Some(true));
        assert_eq!(v.get("d").unwrap(), &Value::Bytes(b"hi".to_vec()));
    }
}
