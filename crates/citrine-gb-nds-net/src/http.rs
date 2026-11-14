#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub method: String,
    pub target: String,
    pub host: String,
    pub body: Vec<u8>,
}

impl Request {
    pub fn path(&self) -> &str {
        self.target.split('?').next().unwrap_or_default()
    }
}

const HEADER_LIMIT: usize = 16 * 1024;
const BODY_LIMIT: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Parsed {
    Incomplete,
    Broken,
    Done(Request, usize),
}

pub fn parse(bytes: &[u8]) -> Parsed {
    let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") else {
        return if bytes.len() > HEADER_LIMIT {
            Parsed::Broken
        } else {
            Parsed::Incomplete
        };
    };
    let Ok(head) = std::str::from_utf8(&bytes[..end]) else {
        return Parsed::Broken;
    };
    let mut lines = head.split("\r\n");
    let mut first = lines.next().unwrap_or_default().split(' ');
    let (Some(method), Some(target)) = (first.next(), first.next()) else {
        return Parsed::Broken;
    };
    let mut host = String::new();
    let mut length = 0;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        if name.eq_ignore_ascii_case("host") {
            host = value
                .split(':')
                .next()
                .unwrap_or_default()
                .to_ascii_lowercase();
        } else if name.eq_ignore_ascii_case("content-length") {
            let Ok(parsed) = value.parse::<usize>() else {
                return Parsed::Broken;
            };
            length = parsed;
        }
    }
    if length > BODY_LIMIT {
        return Parsed::Broken;
    }
    let start = end + 4;
    let Some(body) = bytes.get(start..start + length) else {
        return Parsed::Incomplete;
    };
    Parsed::Done(
        Request {
            method: method.to_owned(),
            target: target.to_owned(),
            host,
            body: body.to_vec(),
        },
        start + length,
    )
}

pub fn response(status: u16, headers: &[(&str, &str)], body: &[u8]) -> Vec<u8> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        502 => "Bad Gateway",
        _ => "Error",
    };
    let mut head = format!("HTTP/1.1 {status} {reason}\r\n");
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str(&format!(
        "Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    ));
    let mut bytes = head.into_bytes();
    bytes.extend_from_slice(body);
    bytes
}

pub fn form(body: &[u8]) -> Vec<(String, String)> {
    String::from_utf8_lossy(body)
        .split('&')
        .filter_map(|pair| {
            let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
            (!name.is_empty()).then(|| (unescape(name), unescape(value)))
        })
        .collect()
}

fn unescape(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'%' if at + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[at + 1..at + 3]).unwrap_or_default();
                match u8::from_str_radix(hex, 16) {
                    Ok(byte) => {
                        out.push(byte);
                        at += 3;
                    }
                    Err(_) => {
                        out.push(b'%');
                        at += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                at += 1;
            }
            byte => {
                out.push(byte);
                at += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_post_waits_for_its_whole_body() {
        let head =
            b"POST /ac HTTP/1.1\r\nHost: nas.nintendowifi.net\r\nContent-Length: 5\r\n\r\nab";
        assert_eq!(parse(head), Parsed::Incomplete);
        let mut whole = head.to_vec();
        whole.extend_from_slice(b"cde");
        let Parsed::Done(request, used) = parse(&whole) else {
            panic!("not parsed");
        };
        assert_eq!(used, whole.len());
        assert_eq!(request.method, "POST");
        assert_eq!(request.host, "nas.nintendowifi.net");
        assert_eq!(request.body, b"abcde");
    }

    #[test]
    fn a_get_keeps_its_path_and_host() {
        let Parsed::Done(request, _) =
            parse(b"GET /a/info.asp?pid=7 HTTP/1.1\r\nHost: X.example:80\r\n\r\n")
        else {
            panic!("not parsed");
        };
        assert_eq!(request.path(), "/a/info.asp");
        assert_eq!(request.host, "x.example");
    }

    #[test]
    fn a_form_body_unescapes_its_values() {
        assert_eq!(
            form(b"action=bG9naW4*&ingamesn=a%2Ab+c&empty="),
            vec![
                ("action".to_owned(), "bG9naW4*".to_owned()),
                ("ingamesn".to_owned(), "a*b c".to_owned()),
                ("empty".to_owned(), String::new()),
            ]
        );
    }

    #[test]
    fn a_response_carries_its_length() {
        let bytes = response(200, &[("X-Organization", "Nintendo")], b"ok");
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(text.contains("X-Organization: Nintendo\r\n"));
        assert!(text.ends_with("Content-Length: 2\r\nConnection: close\r\n\r\nok"));
    }
}
