//! The local listener the browser comes back to after logging in, and what its call says.

use std::{
    io::{Read as _, Write as _},
    net::{Ipv4Addr, Ipv6Addr, TcpListener, TcpStream},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use reqwest::Url;

use super::LOGIN_TIMEOUT;

const POLL: Duration = Duration::from_millis(100);

/// What the browser brought back to the callback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Callback {
    /// Not the callback path, such as the browser asking for a favicon.
    Other,
    /// A callback for another login, such as an old tab reloaded; this login waits on.
    Stray,
    Code(String),
    Refused(String),
}

/// Read the request line of a call to the listener.
pub(super) fn parse_callback(request_line: &str, expected_state: &str) -> Callback {
    let mut words = request_line.split_whitespace();
    let (Some("GET"), Some(target)) = (words.next(), words.next()) else {
        return Callback::Other;
    };
    let Ok(url) = Url::parse(&format!("http://localhost{target}")) else {
        return Callback::Other;
    };
    if url.path().trim_end_matches('/') != "/callback" {
        return Callback::Other;
    }
    let param = |name: &str| {
        url.query_pairs()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.into_owned())
    };
    // Only an answer carrying this login's state ends it; anything else is left alone, so a
    // stray request cannot cut the login short.
    if param("state").as_deref() != Some(expected_state) {
        return Callback::Stray;
    }
    if let Some(error) = param("error") {
        let description = param("error_description").unwrap_or_default();
        return Callback::Refused(if description.is_empty() {
            format!("EVE SSO refused the login: {error}")
        } else {
            format!("EVE SSO refused the login: {description}")
        });
    }
    match param("code") {
        Some(code) if !code.is_empty() => Callback::Code(code),
        _ => Callback::Refused("EVE SSO sent no code".to_owned()),
    }
}

/// The local listener the browser comes back to, on IPv4 and IPv6 localhost both, since
/// "localhost" can be either.
pub(super) struct Listener {
    sockets: Vec<TcpListener>,
}

impl Listener {
    /// Listen on `port`, waiting a moment for a login that was just cancelled to let go of it.
    pub fn bind(port: u16) -> Result<Self, String> {
        let mut tries = 0;
        loop {
            match Self::bind_once(port) {
                Err(_) if tries < 10 => {
                    tries += 1;
                    std::thread::sleep(POLL * 2);
                }
                result => return result,
            }
        }
    }

    fn bind_once(port: u16) -> Result<Self, String> {
        let attempts = [
            TcpListener::bind((Ipv4Addr::LOCALHOST, port)),
            TcpListener::bind((Ipv6Addr::LOCALHOST, port)),
        ];
        let mut sockets = Vec::new();
        let mut error = None;
        for attempt in attempts {
            match attempt {
                Ok(socket) => sockets.push(socket),
                Err(e) => error = Some(e),
            }
        }
        if sockets.is_empty() {
            let e = error.map_or_else(String::new, |e| e.to_string());
            return Err(format!("cannot listen on port {port} for the login: {e}"));
        }
        for socket in &sockets {
            socket
                .set_nonblocking(true)
                .map_err(|e| format!("login listener: {e}"))?;
        }
        Ok(Self { sockets })
    }

    /// Wait for the browser to come back with a code, until `cancel` is set or the time runs
    /// out.
    pub fn wait(&self, state: &str, cancel: &AtomicBool) -> Result<String, String> {
        let deadline = Instant::now() + LOGIN_TIMEOUT;
        while Instant::now() < deadline {
            if cancel.load(Ordering::Relaxed) {
                return Err("login cancelled".to_owned());
            }
            for socket in &self.sockets {
                match socket.accept() {
                    Ok((stream, _)) => match answer(stream, state) {
                        Callback::Other | Callback::Stray => {}
                        Callback::Code(code) => return Ok(code),
                        Callback::Refused(e) => return Err(e),
                    },
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(e) => return Err(format!("login listener: {e}")),
                }
            }
            std::thread::sleep(POLL);
        }
        Err("the login timed out".to_owned())
    }
}

/// Read one request, answer it with a short page, and say what it was.
fn answer(mut stream: TcpStream, state: &str) -> Callback {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut request = Vec::new();
    let mut buf = [0u8; 2048];
    while !request.windows(4).any(|w| w == b"\r\n\r\n") && request.len() < 16 * 1024 {
        match stream.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => request.extend_from_slice(&buf[..n]),
        }
    }
    let text = String::from_utf8_lossy(&request);
    let callback = parse_callback(text.lines().next().unwrap_or_default(), state);
    let (status, message) = match &callback {
        Callback::Other => ("404 Not Found", "Nothing here.".to_owned()),
        Callback::Stray => (
            "400 Bad Request",
            "This answer belongs to another login. Finish the login in the tab efa opened last."
                .to_owned(),
        ),
        Callback::Code(_) => (
            "200 OK",
            "Logged in. You can close this tab and go back to efa.".to_owned(),
        ),
        Callback::Refused(e) => ("400 Bad Request", format!("The login failed: {e}")),
    };
    let body = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>efa</title></head>\
         <body style=\"font-family:sans-serif;margin:3em\"><p>{}</p></body></html>",
        crate::port::escape_markup(&message)
    );
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\
         Connection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.flush();
    callback
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_callback() {
        assert_eq!(
            parse_callback("GET /callback?code=xyz&state=s1 HTTP/1.1", "s1"),
            Callback::Code("xyz".to_owned())
        );
        assert_eq!(
            parse_callback("GET /favicon.ico HTTP/1.1", "s1"),
            Callback::Other
        );
        assert_eq!(
            parse_callback("GET /callback?code=xyz&state=other HTTP/1.1", "s1"),
            Callback::Stray
        );
        assert_eq!(
            parse_callback("GET /callback?error=access_denied&state=old HTTP/1.1", "s1"),
            Callback::Stray,
            "a refusal for another login does not end this one"
        );
        assert_eq!(
            parse_callback("GET /callback?code=xyz HTTP/1.1", "s1"),
            Callback::Stray
        );
        assert!(matches!(
            parse_callback("GET /callback?error=access_denied&state=s1 HTTP/1.1", "s1"),
            Callback::Refused(e) if e.contains("access_denied")
        ));
    }
}
