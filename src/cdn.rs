//! Client art straight off CCP's content CDN, found the way the launcher finds it:
//! `eveclient_TQ.json` names the build, `eveonline_<build>.txt` points at the resource index, and
//! the resource index maps every `res:/` path to a content hash served by resources.eveonline.com.
//!
//! The resource index is about 20 MB, so it is kept on disk per build.

use std::{
    collections::HashMap,
    io::{BufRead, BufReader},
    path::Path,
};

const BINARIES: &str = "https://binaries.eveonline.com";
const RESOURCES: &str = "https://resources.eveonline.com";
const AGENT: &str = "gyfa/0.1";

pub struct Cdn {
    http: reqwest::blocking::Client,
    /// Lowercased `res:/` path to content hash. Only art is kept.
    index: HashMap<String, String>,
}

impl Cdn {
    pub fn connect(cache_dir: &Path) -> Result<Self, String> {
        #[derive(serde::Deserialize)]
        struct ClientInfo {
            build: String,
        }

        let http = reqwest::blocking::Client::builder()
            .user_agent(AGENT)
            .build()
            .map_err(|e| e.to_string())?;
        let info: ClientInfo =
            serde_json::from_reader(get(&http, &format!("{BINARIES}/eveclient_TQ.json"))?)
                .map_err(|e| format!("client info: {e}"))?;

        let cached = cache_dir.join(format!("resfileindex-{}.txt", info.build));
        if !cached.exists() {
            let app_index = get(&http, &format!("{BINARIES}/eveonline_{}.txt", info.build))?;
            let res_index = BufReader::new(app_index)
                .lines()
                .map_while(Result::ok)
                .find_map(|line| {
                    let mut fields = line.split(',');
                    (fields.next()? == "app:/resfileindex.txt")
                        .then(|| fields.next().map(str::to_owned))?
                })
                .ok_or("the client index has no resource index")?;
            let bytes = get(&http, &format!("{BINARIES}/{res_index}"))?
                .bytes()
                .map_err(|e| format!("resource index: {e}"))?;
            std::fs::create_dir_all(cache_dir).map_err(|e| e.to_string())?;
            std::fs::write(&cached, &bytes).map_err(|e| format!("{}: {e}", cached.display()))?;
        }

        let text =
            std::fs::read_to_string(&cached).map_err(|e| format!("{}: {e}", cached.display()))?;
        let index = text
            .lines()
            .filter_map(|line| {
                let mut fields = line.split(',');
                let path = fields.next()?.to_lowercase();
                let wanted = path.starts_with("res:/ui/") || path.contains("/icons/");
                wanted.then(|| Some((path, fields.next()?.to_owned())))?
            })
            .collect();
        Ok(Self { http, index })
    }

    pub fn fetch(&self, res_path: &str) -> Result<Vec<u8>, String> {
        let hash = self
            .index
            .get(&res_path.to_lowercase())
            .ok_or_else(|| format!("{res_path} is not in the client"))?;
        Ok(get(&self.http, &format!("{RESOURCES}/{hash}"))?
            .bytes()
            .map_err(|e| format!("{res_path}: {e}"))?
            .to_vec())
    }
}

fn get(http: &reqwest::blocking::Client, url: &str) -> Result<reqwest::blocking::Response, String> {
    http.get(url)
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .map_err(|e| format!("{url}: {e}"))
}
