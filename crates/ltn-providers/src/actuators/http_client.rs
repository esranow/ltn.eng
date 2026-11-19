use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status_code: u16,
    pub status_text: String,
    pub body: String,
}

pub struct MinimalHttpClient;

impl MinimalHttpClient {
    pub fn request(
        method: &str,
        url_str: &str,
        headers: &[(&str, &str)],
        body: Option<&str>,
    ) -> Result<HttpResponse, String> {
        if url_str.starts_with("https://") {
            return Self::request_https(method, url_str, headers, body);
        }

        let (host, port, path) = Self::parse_url(url_str)?;
        let addr = format!("{}:{}", host, port);

        let socket_addrs: Vec<_> = addr
            .to_socket_addrs()
            .map_err(|e| format!("DNS/Address resolution failed for {}: {}", addr, e))?
            .collect();

        if socket_addrs.is_empty() {
            return Err(format!("No IP address found for {}", addr));
        }

        let mut connected_stream = None;
        let mut last_err = String::new();
        for saddr in &socket_addrs {
            match TcpStream::connect_timeout(saddr, Duration::from_secs(3)) {
                Ok(s) => {
                    connected_stream = Some(s);
                    break;
                }
                Err(e) => {
                    last_err = format!("Failed to connect to {}: {}", saddr, e);
                }
            }
        }

        let mut stream = connected_stream.ok_or_else(|| {
            if last_err.is_empty() {
                format!("Failed to connect to {}", addr)
            } else {
                last_err
            }
        })?;

        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .map_err(|e| format!("Failed to set read timeout: {}", e))?;
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .map_err(|e| format!("Failed to set write timeout: {}", e))?;

        let body_bytes = body.unwrap_or("").as_bytes();
        let content_length = body_bytes.len();

        let mut req_str = format!(
            "{} {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n",
            method, path, host
        );
        let mut has_content_type = false;
        let mut has_content_length = false;

        for (k, v) in headers {
            if k.eq_ignore_ascii_case("content-type") {
                has_content_type = true;
            }
            if k.eq_ignore_ascii_case("content-length") {
                has_content_length = true;
            }
            req_str.push_str(&format!("{}: {}\r\n", k, v));
        }

        if !has_content_type && !body_bytes.is_empty() {
            req_str.push_str("Content-Type: application/json\r\n");
        }
        if !has_content_length {
            req_str.push_str(&format!("Content-Length: {}\r\n", content_length));
        }
        req_str.push_str("\r\n");

        stream
            .write_all(req_str.as_bytes())
            .map_err(|e| format!("Failed to write HTTP headers to {}: {}", addr, e))?;

        if !body_bytes.is_empty() {
            stream
                .write_all(body_bytes)
                .map_err(|e| format!("Failed to write HTTP body to {}: {}", addr, e))?;
        }

        stream
            .flush()
            .map_err(|e| format!("Failed to flush stream: {}", e))?;

        let mut raw_response = Vec::new();
        stream
            .read_to_end(&mut raw_response)
            .map_err(|e| format!("Failed to read HTTP response from {}: {}", addr, e))?;

        let response_str = String::from_utf8_lossy(&raw_response).to_string();
        Self::parse_response(&response_str)
    }

    fn parse_url(url: &str) -> Result<(String, u16, String), String> {
        let (scheme, rest) = if let Some(stripped) = url.strip_prefix("http://") {
            ("http", stripped)
        } else if let Some(stripped) = url.strip_prefix("https://") {
            ("https", stripped)
        } else {
            ("http", url)
        };

        let (host_port, path) = match rest.find('/') {
            Some(idx) => (&rest[..idx], &rest[idx..]),
            None => (rest, "/"),
        };

        let (host, port) = if let Some(idx) = host_port.find(':') {
            let h = host_port[..idx].to_string();
            let p: u16 = host_port[idx + 1..]
                .parse()
                .map_err(|_| format!("Invalid port in URL: {}", url))?;
            (h, p)
        } else {
            let default_port = if scheme == "https" { 443 } else { 80 };
            (host_port.to_string(), default_port)
        };

        Ok((host, port, path.to_string()))
    }

    fn request_https(
        method: &str,
        url_str: &str,
        headers: &[(&str, &str)],
        body: Option<&str>,
    ) -> Result<HttpResponse, String> {
        let mut cmd = std::process::Command::new("curl.exe");
        cmd.arg("-s").arg("-i").arg("-X").arg(method);
        for (k, v) in headers {
            cmd.arg("-H").arg(format!("{}: {}", k, v));
        }
        if let Some(b) = body {
            cmd.arg("-d").arg(b);
        }
        cmd.arg(url_str);

        let output = cmd
            .output()
            .or_else(|_| {
                let mut fallback = std::process::Command::new("curl");
                fallback.arg("-s").arg("-i").arg("-X").arg(method);
                for (k, v) in headers {
                    fallback.arg("-H").arg(format!("{}: {}", k, v));
                }
                if let Some(b) = body {
                    fallback.arg("-d").arg(b);
                }
                fallback.arg(url_str);
                fallback.output()
            })
            .map_err(|e| {
                format!(
                    "Failed to execute curl for HTTPS request to {}: {}",
                    url_str, e
                )
            })?;

        let raw_str = String::from_utf8_lossy(&output.stdout).to_string();
        if raw_str.is_empty() {
            let err_str = String::from_utf8_lossy(&output.stderr).to_string();
            return Err(format!(
                "HTTPS request via curl returned empty response: {}",
                err_str
            ));
        }

        Self::parse_response(&raw_str)
    }

    fn parse_response(response_str: &str) -> Result<HttpResponse, String> {
        // If there are multiple HTTP response headers (e.g. 100 Continue, 301 Redirect), find the last status line
        let norm = response_str.replace("\r\n", "\n");
        let (header_chunk, body_chunk) = if let Some(idx) = norm.find("\n\n") {
            (&norm[..idx], &norm[idx + 2..])
        } else {
            (norm.as_str(), "")
        };

        let mut status_line = "";
        for line in header_chunk.lines() {
            if line.starts_with("HTTP/") {
                status_line = line;
            }
        }
        if status_line.is_empty() {
            status_line = header_chunk.lines().next().unwrap_or("");
        }

        let mut status_parts = status_line.splitn(3, ' ');
        let _version = status_parts
            .next()
            .ok_or_else(|| format!("Invalid HTTP status line: {}", status_line))?;
        let status_code_str = status_parts
            .next()
            .ok_or_else(|| format!("Missing status code in: {}", status_line))?;
        let status_text = status_parts.next().unwrap_or("").to_string();

        let status_code: u16 = status_code_str.parse().map_err(|_| {
            format!(
                "Invalid status code '{}' in: {}",
                status_code_str, status_line
            )
        })?;

        Ok(HttpResponse {
            status_code,
            status_text,
            body: body_chunk.to_string(),
        })
    }
}
