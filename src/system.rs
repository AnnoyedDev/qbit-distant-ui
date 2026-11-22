use std::{
    io::{BufRead, BufReader, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    thread,
    time::Duration,
};

const ADDR: &str = if cfg!(test) { "127.0.0.1:47392" } else { "127.0.0.1:47391" };
const MAGIC: &str = "QBITUI1";

pub fn is_url(s: &str) -> bool {
    let l = s.to_ascii_lowercase();
    l.starts_with("magnet:") || l.starts_with("http://") || l.starts_with("https://")
}

fn is_torrent_path(s: &str) -> bool {
    s.to_ascii_lowercase().ends_with(".torrent")
}

fn valid(s: &str) -> bool {
    is_url(s) || is_torrent_path(s)
}

pub fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Ok(v) = u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("x"), 16)
        {
            out.push(v);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn normalize(arg: &str) -> String {
    if is_url(arg) {
        return arg.to_string();
    }
    let path = match arg.strip_prefix("file://") {
        Some(p) => percent_decode(p),
        None => arg.to_string(),
    };
    std::fs::canonicalize(&path).map(|p| p.to_string_lossy().into_owned()).unwrap_or(path)
}

pub fn cli_items() -> Vec<String> {
    std::env::args().skip(1).map(|a| normalize(&a)).filter(|a| valid(a)).collect()
}

pub fn forward(items: &[String]) -> bool {
    let addr: SocketAddr = ADDR.parse().unwrap();
    let Ok(mut s) = TcpStream::connect_timeout(&addr, Duration::from_millis(300)) else {
        return false;
    };
    let _ = s.set_read_timeout(Some(Duration::from_millis(800)));
    if writeln!(s, "{MAGIC}").is_err() {
        return false;
    }
    let mut reply = String::new();
    if BufReader::new(&s).read_line(&mut reply).is_err() || reply.trim() != "OK" {
        return false;
    }
    items.iter().all(|i| writeln!(s, "{i}").is_ok())
}

pub fn listen(on_items: impl Fn(Vec<String>) + Send + 'static) {
    let Ok(listener) = TcpListener::bind(ADDR) else {
        return;
    };
    thread::spawn(move || {
        for s in listener.incoming().flatten() {
            let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
            let mut r = BufReader::new(&s);
            let mut magic = String::new();
            if r.read_line(&mut magic).is_err() || magic.trim() != MAGIC {
                continue;
            }
            if (&s).write_all(b"OK\n").is_err() {
                continue;
            }
            let items: Vec<String> = r.lines().map_while(Result::ok).map(|l| l.trim().to_string()).filter(|l| valid(l)).collect();
            on_items(items);
        }
    });
}

#[cfg(any(target_os = "linux", test))]
const MIMES: [&str; 2] = ["x-scheme-handler/magnet", "application/x-bittorrent"];

#[cfg(any(target_os = "linux", test))]
pub fn set_defaults(ini: &str, desktop: &str, mimes: &[&str]) -> String {
    let entries: Vec<String> = mimes.iter().map(|m| format!("{m}={desktop}")).collect();
    let mut out: Vec<String> = Vec::new();
    let (mut in_section, mut done) = (false, false);
    for line in ini.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            if in_section && !done {
                while out.last().is_some_and(|l| l.trim().is_empty()) {
                    out.pop();
                }
                out.extend(entries.iter().cloned());
                out.push(String::new());
                done = true;
            }
            in_section = t == "[Default Applications]";
        } else if in_section && t.split_once('=').is_some_and(|(k, _)| mimes.contains(&k.trim())) {
            continue;
        }
        out.push(line.to_string());
    }
    if in_section && !done {
        out.extend(entries.iter().cloned());
        done = true;
    }
    if !done {
        if out.last().is_some_and(|l| !l.trim().is_empty()) {
            out.push(String::new());
        }
        out.push("[Default Applications]".into());
        out.extend(entries);
    }
    out.join("\n") + "\n"
}

#[cfg(target_os = "linux")]
pub fn associate() -> Result<(), String> {
    use std::{path::PathBuf, process::{Command, Stdio}};
    let home = std::env::var_os("HOME").map(PathBuf::from).ok_or("HOME ?")?;
    let data = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".local/share"));
    let config = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".config"));
    let apps = data.join("applications");
    std::fs::create_dir_all(&apps).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&config).map_err(|e| e.to_string())?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let desktop = format!(
        "[Desktop Entry]\nType=Application\nName={}\nExec=\"{}\" %u\nIcon={}\nMimeType={};\nTerminal=false\nCategories=Network;FileTransfer;P2P;\n",
        crate::APP_NAME,
        exe.display(),
        crate::APP_ID,
        MIMES.join(";")
    );
    let icons = data.join("icons/hicolor/256x256/apps");
    std::fs::create_dir_all(&icons).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(icons.join("qbit-ui.png"));
    let _ = std::fs::remove_file(apps.join("qbit-ui.desktop"));
    std::fs::write(icons.join(format!("{}.png", crate::APP_ID)), include_bytes!("../assets/icon-256.png")).map_err(|e| e.to_string())?;
    std::fs::write(apps.join(format!("{}.desktop", crate::APP_ID)), desktop).map_err(|e| e.to_string())?;
    let list = config.join("mimeapps.list");
    let current = std::fs::read_to_string(&list).unwrap_or_default();
    std::fs::write(&list, set_defaults(&current, &format!("{}.desktop", crate::APP_ID), &MIMES)).map_err(|e| e.to_string())?;
    for (cmd, args) in [("update-desktop-database", vec![apps.as_os_str()]), ("kbuildsycoca6", vec![]), ("kbuildsycoca5", vec![])] {
        let _ = Command::new(cmd).args(args).stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
    Ok(())
}

#[cfg(windows)]
pub fn associate() -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let cmd = format!("\"{}\" \"%1\"", exe.display());
    let entries: [&[&str]; 6] = [
        &["HKCU\\Software\\Classes\\magnet", "/ve", "/d", "URL:Magnet"],
        &["HKCU\\Software\\Classes\\magnet", "/v", "URL Protocol", "/d", ""],
        &["HKCU\\Software\\Classes\\magnet\\shell\\open\\command", "/ve", "/d", &cmd],
        &["HKCU\\Software\\Classes\\.torrent", "/ve", "/d", "qbitdistantui.torrent"],
        &["HKCU\\Software\\Classes\\qbitdistantui.torrent", "/ve", "/d", "Torrent"],
        &["HKCU\\Software\\Classes\\qbitdistantui.torrent\\shell\\open\\command", "/ve", "/d", &cmd],
    ];
    for e in entries {
        let ok = std::process::Command::new("reg")
            .arg("add")
            .args(e)
            .arg("/f")
            .creation_flags(0x0800_0000)
            .status()
            .map_err(|e| e.to_string())?
            .success();
        if !ok {
            return Err(format!("reg add {}", e[0]));
        }
    }
    Ok(())
}

#[cfg(not(any(target_os = "linux", windows)))]
pub fn associate() -> Result<(), String> {
    Err("err_assoc".into())
}

fn secret_entry(key: &str) -> Option<keyring::Entry> {
    keyring::Entry::new(crate::APP_ID, key).ok()
}

pub fn secret_get(key: &str) -> Option<String> {
    secret_entry(key)?.get_password().ok()
}

pub fn secret_set(key: &str, pass: &str) -> bool {
    let Some(e) = secret_entry(key) else { return false };
    if pass.is_empty() {
        matches!(e.delete_credential(), Ok(()) | Err(keyring::Error::NoEntry))
    } else {
        e.set_password(pass).is_ok()
    }
}

pub fn secret_delete(key: &str) {
    if let Some(e) = secret_entry(key) {
        let _ = e.delete_credential();
    }
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn b64_encode(s: &str) -> String {
    let mut out = String::new();
    for c in s.as_bytes().chunks(3) {
        let n = c.iter().enumerate().fold(0u32, |n, (i, b)| n | (*b as u32) << (16 - 8 * i));
        for i in 0..4 {
            out.push(if i <= c.len() { B64[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
        }
    }
    out
}

pub fn b64_decode(s: &str) -> Option<String> {
    let mut bytes = Vec::new();
    for c in s.trim_end_matches('=').as_bytes().chunks(4) {
        let n = c.iter().enumerate().try_fold(0u32, |n, (i, b)| Some(n | (B64.iter().position(|x| x == b)? as u32) << (18 - 6 * i)))?;
        bytes.extend((0..c.len().saturating_sub(1)).map(|i| (n >> (16 - 8 * i)) as u8));
    }
    String::from_utf8(bytes).ok()
}

pub fn notify(title: &str, body: &str) {
    let _ = notify_rust::Notification::new().appname(crate::APP_NAME).summary(title).body(body).icon(crate::APP_ID).show();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_roundtrip() {
        for t in ["", "f", "fo", "foo", "foob", "fooba", "foobar", "mot de passe é 日本 !"] {
            assert_eq!(b64_decode(&b64_encode(t)).as_deref(), Some(t));
        }
        assert_eq!(b64_encode("foobar"), "Zm9vYmFy");
        assert_eq!(b64_encode("fo"), "Zm8=");
        assert_eq!(b64_decode("Zm9vYg=="), Some("foob".into()));
        assert_eq!(b64_decode("@@@@"), None);
    }

    #[test]
    #[ignore]
    fn real_keyring_roundtrip() {
        let key = "test|qbit-distant-ui-roundtrip";
        assert!(secret_set(key, "s3cr3t é"));
        assert_eq!(secret_get(key).as_deref(), Some("s3cr3t é"));
        assert!(secret_set(key, ""));
        assert_eq!(secret_get(key), None);
    }

    #[test]
    fn mimeapps_editing() {
        let m = ["x-scheme-handler/magnet", "application/x-bittorrent"];
        let before = "[Added Associations]\ntext/plain=kate.desktop;\n\n[Default Applications]\napplication/x-bittorrent=org.qbittorrent.qBittorrent.desktop\ntext/html=firefox.desktop\n\n[Removed Associations]\n";
        let after = set_defaults(before, "qbit-ui.desktop", &m);
        assert_eq!(after, "[Added Associations]\ntext/plain=kate.desktop;\n\n[Default Applications]\ntext/html=firefox.desktop\nx-scheme-handler/magnet=qbit-ui.desktop\napplication/x-bittorrent=qbit-ui.desktop\n\n[Removed Associations]\n");
        assert_eq!(set_defaults(&after, "qbit-ui.desktop", &m), after);
        assert_eq!(set_defaults("", "a.desktop", &m), "[Default Applications]\nx-scheme-handler/magnet=a.desktop\napplication/x-bittorrent=a.desktop\n");
        assert_eq!(set_defaults("[Default Applications]\nx-scheme-handler/magnet=old.desktop", "a.desktop", &m), "[Default Applications]\nx-scheme-handler/magnet=a.desktop\napplication/x-bittorrent=a.desktop\n");
    }

    #[test]
    #[ignore]
    fn real_notification() {
        notify(crate::APP_NAME, "Test de notification ✔");
    }

    #[test]
    fn inputs() {
        assert!(valid("magnet:?xt=urn:btih:abc"));
        assert!(valid("/tmp/a b.TORRENT"));
        assert!(!valid("/etc/passwd"));
        assert_eq!(percent_decode("/tmp/a%20b%C3%A9.torrent"), "/tmp/a bé.torrent");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(normalize("file:///nope/x%20y.torrent"), "/nope/x y.torrent");
        assert_eq!(normalize("magnet:?xt=1"), "magnet:?xt=1");
    }

    #[test]
    fn single_instance_handoff() {
        let (tx, rx) = std::sync::mpsc::channel();
        listen(move |items| tx.send(items).unwrap());
        thread::sleep(Duration::from_millis(100));
        assert!(forward(&["magnet:?xt=urn:btih:abc".into(), "/etc/passwd".into(), "/tmp/x.torrent".into()]));
        let got = rx.recv_timeout(Duration::from_secs(3)).unwrap();
        assert_eq!(got, vec!["magnet:?xt=urn:btih:abc".to_string(), "/tmp/x.torrent".to_string()]);
        assert!(forward(&[]));
        assert!(rx.recv_timeout(Duration::from_secs(3)).unwrap().is_empty());
        let stranger = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = stranger.local_addr().unwrap();
        thread::spawn(move || {
            for s in stranger.incoming().flatten() {
                let _ = (&s).write_all(b"HTTP/1.1 400 Bad Request\r\n\r\n");
            }
        });
        let mut s = TcpStream::connect(addr).unwrap();
        let _ = writeln!(s, "{MAGIC}");
        let mut reply = String::new();
        let _ = BufReader::new(&s).read_line(&mut reply);
        assert_ne!(reply.trim(), "OK");
    }

    #[test]
    #[ignore]
    fn associate_in_sandbox() {
        let out = associate();
        println!("{out:?}");
        assert!(out.is_ok());
    }
}
