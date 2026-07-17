//! Trust-boundary heuristics: flag files that touch the outside world.
//! Deliberately coarse — the map marks *where to look*, an auditor decides.

pub const NETWORK: u8 = 1 << 0;
pub const SUBPROCESS: u8 = 1 << 1;
pub const SECRETS: u8 = 1 << 2;
pub const DANGEROUS_EVAL: u8 = 1 << 3;

pub fn scan(content: &str) -> u8 {
    let mut flags = 0u8;
    let lower = if content.len() <= 512 * 1024 {
        content
    } else {
        &content[..512 * 1024]
    };
    for line in lower.lines() {
        let l = line.trim_start();
        if l.starts_with("//") || l.starts_with('#') || l.starts_with('*') {
            continue;
        }
        if flags & NETWORK == 0 && NETWORK_PATTERNS.iter().any(|p| l.contains(p)) {
            flags |= NETWORK;
        }
        if flags & SUBPROCESS == 0 && SUBPROCESS_PATTERNS.iter().any(|p| l.contains(p)) {
            flags |= SUBPROCESS;
        }
        if flags & SECRETS == 0 && SECRET_PATTERNS.iter().any(|p| l.contains(p)) {
            flags |= SECRETS;
        }
        if flags & DANGEROUS_EVAL == 0 && EVAL_PATTERNS.iter().any(|p| l.contains(p)) {
            flags |= DANGEROUS_EVAL;
        }
        if flags == NETWORK | SUBPROCESS | SECRETS | DANGEROUS_EVAL {
            break;
        }
    }
    flags
}

pub fn tags(flags: u8) -> Vec<&'static str> {
    let mut v = Vec::new();
    if flags & NETWORK != 0 {
        v.push("net");
    }
    if flags & SUBPROCESS != 0 {
        v.push("exec");
    }
    if flags & SECRETS != 0 {
        v.push("secrets");
    }
    if flags & DANGEROUS_EVAL != 0 {
        v.push("eval");
    }
    v
}

const NETWORK_PATTERNS: &[&str] = &[
    "reqwest::",
    "hyper::",
    "TcpListener",
    "TcpStream",
    "UdpSocket",
    "requests.get",
    "requests.post",
    "urllib.request",
    "http.client",
    "aiohttp",
    "fetch(",
    "axios",
    "XMLHttpRequest",
    "http.Get(",
    "http.Post(",
    "net.Listen",
    "HttpClient",
    "HttpURLConnection",
    "socket.socket",
    "websocket",
];

const SUBPROCESS_PATTERNS: &[&str] = &[
    "Command::new",
    "subprocess.",
    "os.system",
    "child_process",
    "execSync",
    "spawnSync",
    "exec.Command",
    "Runtime.getRuntime().exec",
    "ProcessBuilder",
    "popen",
];

const SECRET_PATTERNS: &[&str] = &[
    "API_KEY",
    "SECRET",
    "PASSWORD",
    "PRIVATE_KEY",
    "ACCESS_TOKEN",
    "api_key",
    "client_secret",
    "env::var",
    "os.environ",
    "getenv",
    "process.env",
    "os.Getenv",
    "credentials",
];

const EVAL_PATTERNS: &[&str] = &[
    "pickle.load",
    "yaml.load(",
    "eval(",
    "exec(",
    "deserialize_untrusted",
    "vm.runInContext",
    "Function(",
    "unsafe {",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_network_and_secrets() {
        let f = scan("let key = env::var(\"API_KEY\");\nlet r = reqwest::get(url).await;");
        assert_ne!(f & NETWORK, 0);
        assert_ne!(f & SECRETS, 0);
        assert_eq!(f & SUBPROCESS, 0);
    }

    #[test]
    fn ignores_comments() {
        assert_eq!(scan("// call eval( here\n# os.system"), 0);
    }
}
