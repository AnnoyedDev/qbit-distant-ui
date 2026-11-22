use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{collections::HashMap, sync::Mutex, time::Duration};
use ureq::{Agent, Error};

#[derive(Deserialize, Clone, Default)]
#[serde(default)]
pub struct Torrent {
    pub hash: String,
    pub name: String,
    pub size: i64,
    pub progress: f64,
    pub dlspeed: i64,
    pub upspeed: i64,
    pub state: String,
    pub ratio: f64,
    pub eta: i64,
    pub added_on: i64,
    pub category: String,
    pub save_path: String,
    pub magnet_uri: String,
    pub seq_dl: bool,
    pub f_l_piece_prio: bool,
    pub force_start: bool,
    pub dl_limit: i64,
    pub up_limit: i64,
    pub num_seeds: i64,
    pub num_leechs: i64,
}

#[derive(Deserialize, Clone, Default)]
#[serde(default)]
pub struct Tracker {
    pub url: String,
    pub status: i64,
    pub num_seeds: i64,
    pub num_leeches: i64,
    pub msg: String,
}

#[derive(Deserialize, Clone)]
pub struct TFile {
    pub index: i64,
    pub name: String,
    pub size: i64,
    pub progress: f64,
    pub priority: i64,
}

#[derive(Deserialize)]
pub struct Transfer {
    pub dl_info_speed: i64,
    pub up_info_speed: i64,
}

#[derive(Deserialize, Serialize, Clone, Default, PartialEq)]
#[serde(default)]
pub struct Prefs {
    pub dl_limit: i64,
    pub up_limit: i64,
    pub max_ratio_enabled: bool,
    pub max_ratio: f64,
    pub max_seeding_time_enabled: bool,
    pub max_seeding_time: i64,
    pub max_ratio_act: i64,
}

#[derive(Clone, Default)]
pub struct AddOpts {
    pub save_path: String,
    pub category: String,
    pub start: bool,
}

#[derive(Clone)]
pub enum Source {
    Urls(String),
    File(String, Vec<u8>),
}

pub struct Client {
    base: String,
    user: String,
    pass: String,
    cookie: Mutex<Option<String>>,
    agent: Agent,
}

type Res<T> = Result<T, String>;

impl Client {
    pub fn new(base: &str, user: &str, pass: &str) -> Self {
        let agent = Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(15)))
            .build()
            .into();
        Self {
            base: base.trim().trim_end_matches('/').to_string(),
            user: user.into(),
            pass: pass.into(),
            cookie: Mutex::new(None),
            agent,
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}/api/v2/{path}", self.base)
    }

    pub fn login(&self) -> Res<()> {
        let mut cookie = None;
        let ok = match self
            .agent
            .post(self.url("auth/login"))
            .send_form([("username", &*self.user), ("password", &*self.pass)])
        {
            Ok(mut r) => {
                cookie = session_cookie(r.headers());
                r.status() == 204 || r.body_mut().read_to_string().map_err(explain)?.trim() == "Ok."
            }
            Err(Error::StatusCode(401)) => false,
            Err(e) => return Err(explain(e)),
        };
        if !ok {
            return match self.agent.get(self.url("app/version")).call() {
                Ok(_) => Ok(()),
                Err(_) => Err("err_auth".into()),
            };
        }
        *self.cookie.lock().unwrap() = cookie;
        Ok(())
    }

    fn call(&self, f: impl Fn(Option<String>) -> Result<String, Error>) -> Result<String, Error> {
        match f(self.cookie.lock().unwrap().clone()) {
            Err(Error::StatusCode(403)) => {
                self.login().map_err(|_| Error::StatusCode(401))?;
                f(self.cookie.lock().unwrap().clone())
            }
            r => r,
        }
    }

    fn get(&self, path: &str, q: &[(&str, &str)]) -> Result<String, Error> {
        self.call(|c| {
            let mut rq = self.agent.get(self.url(path)).query_pairs(q.iter().copied());
            if let Some(c) = c {
                rq = rq.header("Cookie", c);
            }
            rq.call()?.body_mut().read_to_string()
        })
    }

    fn post(&self, path: &str, form: &[(&str, &str)]) -> Result<String, Error> {
        self.call(|c| {
            let mut rq = self.agent.post(self.url(path));
            if let Some(c) = c {
                rq = rq.header("Cookie", c);
            }
            rq.send_form(form.to_vec())?.body_mut().read_to_string()
        })
    }

    fn json<T: DeserializeOwned>(&self, path: &str, q: &[(&str, &str)]) -> Res<T> {
        let body = self.get(path, q).map_err(explain)?;
        serde_json::from_str(&body).map_err(|e| e.to_string())
    }

    fn act(&self, path: &str, form: &[(&str, &str)]) -> Res<()> {
        self.post(path, form).map(drop).map_err(explain)
    }

    pub fn torrents(&self) -> Res<Vec<Torrent>> {
        self.json("torrents/info", &[("sort", "added_on"), ("reverse", "true")])
    }

    pub fn transfer(&self) -> Res<Transfer> {
        self.json("transfer/info", &[])
    }

    pub fn alt_speed(&self) -> Res<bool> {
        Ok(self.get("transfer/speedLimitsMode", &[]).map_err(explain)?.trim() == "1")
    }

    pub fn toggle_alt_speed(&self) -> Res<()> {
        self.act("transfer/toggleSpeedLimitsMode", &[])
    }

    pub fn categories(&self) -> Res<Vec<String>> {
        let map: HashMap<String, serde_json::Value> = self.json("torrents/categories", &[])?;
        let mut v: Vec<String> = map.into_keys().collect();
        v.sort_by_key(|c| c.to_lowercase());
        Ok(v)
    }

    pub fn default_save_path(&self) -> Res<String> {
        Ok(self.get("app/defaultSavePath", &[]).map_err(explain)?.trim().to_string())
    }

    pub fn dirs(&self, path: &str) -> Res<Vec<String>> {
        let mut v: Vec<String> = self.json("app/getDirectoryContent", &[("dirPath", path), ("mode", "dirs")])?;
        v.sort_by_key(|d| d.to_lowercase());
        Ok(v)
    }

    pub fn files(&self, hash: &str) -> Res<Vec<TFile>> {
        self.json("torrents/files", &[("hash", hash)])
    }

    pub fn set_file_prio(&self, hash: &str, ids: &[i64], prio: i64) -> Res<()> {
        let ids = ids.iter().map(i64::to_string).collect::<Vec<_>>().join("|");
        self.act("torrents/filePrio", &[("hash", hash), ("id", &ids), ("priority", &prio.to_string())])
    }

    pub fn set_running(&self, hashes: &str, run: bool) -> Res<()> {
        let (v5, v4) = if run { ("start", "resume") } else { ("stop", "pause") };
        match self.post(&format!("torrents/{v5}"), &[("hashes", hashes)]) {
            Err(Error::StatusCode(404)) => self.post(&format!("torrents/{v4}"), &[("hashes", hashes)]),
            r => r,
        }
        .map(drop)
        .map_err(explain)
    }

    pub fn delete(&self, hash: &str, files: bool) -> Res<()> {
        self.act("torrents/delete", &[("hashes", hash), ("deleteFiles", if files { "true" } else { "false" })])
    }

    pub fn recheck(&self, hash: &str) -> Res<()> {
        self.act("torrents/recheck", &[("hashes", hash)])
    }

    pub fn reannounce(&self, hash: &str) -> Res<()> {
        self.act("torrents/reannounce", &[("hashes", hash)])
    }

    pub fn set_location(&self, hash: &str, location: &str) -> Res<()> {
        self.act("torrents/setLocation", &[("hashes", hash), ("location", location)])
    }

    pub fn trackers(&self, hash: &str) -> Res<Vec<Tracker>> {
        let v: Vec<Tracker> = self.json("torrents/trackers", &[("hash", hash)])?;
        Ok(v.into_iter().filter(|t| !t.url.starts_with("** [")).collect())
    }

    pub fn add_trackers(&self, hash: &str, urls: &str) -> Res<()> {
        self.act("torrents/addTrackers", &[("hash", hash), ("urls", urls)])
    }

    pub fn remove_trackers(&self, hash: &str, urls: &str) -> Res<()> {
        self.act("torrents/removeTrackers", &[("hash", hash), ("urls", urls)])
    }

    pub fn free_space(&self) -> Res<i64> {
        let v: serde_json::Value = self.json("sync/maindata", &[("rid", "0")])?;
        Ok(v["server_state"]["free_space_on_disk"].as_i64().unwrap_or(-1))
    }

    pub fn toggle_sequential(&self, hash: &str) -> Res<()> {
        self.act("torrents/toggleSequentialDownload", &[("hashes", hash)])
    }

    pub fn toggle_first_last(&self, hash: &str) -> Res<()> {
        self.act("torrents/toggleFirstLastPiecePrio", &[("hashes", hash)])
    }

    pub fn set_force_start(&self, hash: &str, on: bool) -> Res<()> {
        self.act("torrents/setForceStart", &[("hashes", hash), ("value", if on { "true" } else { "false" })])
    }

    pub fn set_category(&self, hash: &str, category: &str) -> Res<()> {
        self.act("torrents/setCategory", &[("hashes", hash), ("category", category)])
    }

    pub fn rename(&self, hash: &str, name: &str) -> Res<()> {
        self.act("torrents/rename", &[("hash", hash), ("name", name)])
    }

    pub fn set_limits(&self, hash: &str, dl: i64, up: i64) -> Res<()> {
        self.act("torrents/setDownloadLimit", &[("hashes", hash), ("limit", &dl.max(0).to_string())])?;
        self.act("torrents/setUploadLimit", &[("hashes", hash), ("limit", &up.max(0).to_string())])
    }

    pub fn prefs(&self) -> Res<Prefs> {
        self.json("app/preferences", &[])
    }

    pub fn set_prefs(&self, p: &Prefs) -> Res<()> {
        let json = serde_json::to_string(p).map_err(|e| e.to_string())?;
        self.act("app/setPreferences", &[("json", &json)])
    }

    pub fn add(&self, src: &Source, o: &AddOpts) -> Res<()> {
        let mut fields: Vec<(&str, &str)> = Vec::new();
        let mut file = None;
        match src {
            Source::Urls(u) => fields.push(("urls", u)),
            Source::File(name, data) => file = Some((name.as_str(), data.as_slice())),
        }
        if !o.save_path.trim().is_empty() {
            fields.push(("savepath", o.save_path.trim()));
        }
        if !o.category.is_empty() {
            fields.push(("category", &o.category));
        }
        if !o.start {
            match src {
                Source::Urls(_) => fields.push(("stopCondition", "MetadataReceived")),
                Source::File(..) => fields.extend([("stopped", "true"), ("paused", "true")]),
            }
        }
        let (ctype, body) = multipart(&fields, file);
        let body = self
            .call(|c| {
                let mut rq = self.agent.post(self.url("torrents/add")).header("Content-Type", &ctype);
                if let Some(c) = c {
                    rq = rq.header("Cookie", c);
                }
                rq.send(&body[..])?.body_mut().read_to_string()
            })
            .map_err(explain)?;
        if body.trim() == "Fails." {
            return Err("err_rejected".into());
        }
        Ok(())
    }
}

fn session_cookie(headers: &ureq::http::HeaderMap) -> Option<String> {
    let all: Vec<&str> = headers
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok()?.split(';').next())
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .collect();
    (!all.is_empty()).then(|| all.join("; "))
}

fn explain(e: Error) -> String {
    match e {
        Error::StatusCode(401) => "err_auth".into(),
        Error::StatusCode(403) => "err_forbidden".into(),
        Error::StatusCode(404) => "err_notfound".into(),
        Error::StatusCode(409) => "err_rejected".into(),
        Error::Http(_) | Error::BadUri(_) => "err_url".into(),
        Error::HostNotFound => "err_host".into(),
        Error::Timeout(_) => "err_timeout".into(),
        e if e.to_string().contains("certificate") => "err_cert".into(),
        e => e.to_string(),
    }
}

fn multipart(fields: &[(&str, &str)], file: Option<(&str, &[u8])>) -> (String, Vec<u8>) {
    let b = "----qbitui7d1f0c9e";
    let mut body = Vec::new();
    for (k, v) in fields {
        body.extend(format!("--{b}\r\nContent-Disposition: form-data; name=\"{k}\"\r\n\r\n{v}\r\n").as_bytes());
    }
    if let Some((name, data)) = file {
        let name = name.replace(['"', '\r', '\n'], "");
        body.extend(
            format!("--{b}\r\nContent-Disposition: form-data; name=\"torrents\"; filename=\"{name}\"\r\nContent-Type: application/x-bittorrent\r\n\r\n")
                .as_bytes(),
        );
        body.extend(data);
        body.extend(b"\r\n");
    }
    body.extend(format!("--{b}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={b}"), body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multipart_body() {
        let (ct, body) = multipart(&[("urls", "magnet:?xt=abc"), ("savepath", "/dl")], Some(("a\".torrent", b"DATA")));
        let s = String::from_utf8(body).unwrap();
        assert!(ct.ends_with("----qbitui7d1f0c9e"));
        assert!(s.contains("name=\"urls\"\r\n\r\nmagnet:?xt=abc\r\n"));
        assert!(s.contains("name=\"savepath\"\r\n\r\n/dl\r\n"));
        assert!(s.contains("filename=\"a.torrent\""));
        assert!(s.contains("\r\n\r\nDATA\r\n"));
        assert!(s.ends_with("------qbitui7d1f0c9e--\r\n"));
    }

    #[test]
    fn cookies_behind_cloudflare() {
        let mut h = ureq::http::HeaderMap::new();
        h.append("set-cookie", "__cf_bm=abc; path=/; HttpOnly; Secure".parse().unwrap());
        h.append("set-cookie", "SID=xyz; HttpOnly; SameSite=Strict; path=/".parse().unwrap());
        assert_eq!(session_cookie(&h).as_deref(), Some("__cf_bm=abc; SID=xyz"));
        assert_eq!(session_cookie(&ureq::http::HeaderMap::new()), None);
        assert_eq!(explain(Error::StatusCode(401)), "err_auth");
        assert_eq!(Client::new("torrent.example.com", "", "").login().unwrap_err(), "err_url");
    }

    #[test]
    fn parse_torrent() {
        let j = r#"[{"hash":"h","name":"n","size":10,"progress":0.5,"dlspeed":1,"upspeed":2,"state":"stoppedDL","ratio":0.1,"eta":8640000,"extra":1}]"#;
        let t: Vec<Torrent> = serde_json::from_str(j).unwrap();
        assert_eq!(t[0].state, "stoppedDL");
        assert_eq!(t[0].category, "");
    }
}

#[cfg(test)]
#[test]
#[ignore]
fn live_server() {
    let url = std::env::var("QBIT_URL").unwrap_or("http://localhost:8080".into());
    let c = Client::new(&url, &std::env::var("QBIT_USER").unwrap_or_default(), &std::env::var("QBIT_PASS").unwrap_or_default());
    let _ = c.login();
    let t = c.torrents().unwrap();
    c.transfer().unwrap();
    c.alt_speed().unwrap();
    c.categories().unwrap();
    c.prefs().unwrap();
    assert!(!c.default_save_path().unwrap().is_empty());
    assert!(c.free_space().unwrap() > 0);
    if let Some(t) = t.first() {
        assert!(!c.files(&t.hash).unwrap().is_empty());
        c.trackers(&t.hash).unwrap();
        println!("premier : {} ({})", t.name, t.state);
    }
}
