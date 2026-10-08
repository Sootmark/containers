//! Container forensics: what ran in containers, from the host's files.
//!
//! - Docker's container configuration (`/var/lib/docker/containers/<id>/config.v2.json`,
//!   `config.json` before Docker 1.10): the container's id, name, image,
//!   command, user, environment, volumes and ports, when it was created,
//!   started and finished, and whether it runs.
//! - Docker's `json-file` logs (`<id>-json.log`, `container-json.log`):
//!   each line the container wrote, its stream and time.
//! - Docker's image layer configurations (`/var/lib/docker/graph/<id>/json`
//!   before Docker 1.10, `image/<driver>/imagedb/content/sha256/<id>` after):
//!   when each layer was made and the command that made it.
//! - The Container Runtime Interface's logs (Kubernetes, containerd,
//!   CRI-O: `/var/log/pods/…/<n>.log`, `/var/log/containers/*.log`):
//!   `<time> <stream> <P|F> <text>`, a `P` line continued by the next.
//!
//! Times are UTC. Damage goes to `problems`, never a panic.

mod time;

use common::json::{self, Json};
use common::time::Ts;

/// This crate's version, for records of what parsed them.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// A kind of container file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A Docker container configuration.
    DockerContainer,
    /// A Docker `json-file` log.
    DockerLog,
    /// A Docker image layer configuration.
    DockerLayer,
    /// A CRI container log.
    CriLog,
}

/// Which container file this is, by its name and first line.
#[must_use]
pub fn detect(name: &str, head: &[u8]) -> Option<Kind> {
    let path = name.replace('\\', "/").to_ascii_lowercase();
    let base = path.rsplit('/').next().unwrap_or(&path);
    let text = String::from_utf8_lossy(head);
    let first = text.lines().next().unwrap_or_default();
    if (base == "config.v2.json" || base == "config.json") && path.contains("containers/") {
        Some(Kind::DockerContainer)
    } else if base.ends_with("-json.log") && first.starts_with("{\"log\":") {
        Some(Kind::DockerLog)
    } else if (path.contains("docker/graph/") && base == "json")
        || (path.contains("imagedb/content/") && text.contains("\"rootfs\""))
    {
        Some(Kind::DockerLayer)
    } else if cri_line(first).is_some() {
        Some(Kind::CriLog)
    } else {
        None
    }
}

/// A container.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Container {
    /// Its id (64 hexadecimal digits).
    pub id: Option<String>,
    /// Its name (without Docker's leading `/`).
    pub name: Option<String>,
    /// Its host name inside.
    pub hostname: Option<String>,
    /// The image it ran (`Config.Image`, as typed).
    pub image: Option<String>,
    /// The image's id.
    pub image_id: Option<String>,
    /// The command it ran: path and arguments.
    pub command: Option<String>,
    /// The user it ran as.
    pub user: Option<String>,
    /// Its environment (`NAME=value`).
    pub env: Vec<String>,
    /// Its volumes and bind mounts (`source:destination`).
    pub mounts: Vec<String>,
    /// Its published ports (`80/tcp->0.0.0.0:8080`).
    pub ports: Vec<String>,
    /// Whether it was privileged.
    pub privileged: Option<bool>,
    /// When it was created.
    pub created: Option<Ts>,
    /// When it last started.
    pub started: Option<Ts>,
    /// When it last finished.
    pub finished: Option<Ts>,
    /// Whether it was running.
    pub running: Option<bool>,
}

/// A line a container wrote, or a CRI log line.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LogLine {
    /// Its line in the file, from 1.
    pub line: usize,
    /// When.
    pub time: Option<Ts>,
    /// `stdout` or `stderr`.
    pub stream: String,
    /// What was written.
    pub text: String,
    /// A CRI line cut (`P`): the next line goes on with it.
    pub partial: bool,
}

/// An image layer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Layer {
    /// Its id, when the file says.
    pub id: Option<String>,
    /// When it was made.
    pub created: Option<Ts>,
    /// The command that made it (`container_config.Cmd` joined).
    pub command: Option<String>,
    /// Who made it.
    pub author: Option<String>,
}

/// What a file holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Content {
    /// A container's configuration.
    Container(Box<Container>),
    /// Log lines.
    Log(Vec<LogLine>),
    /// A layer's configuration.
    Layer(Layer),
}

/// What a file holds, and what couldn't be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Read {
    /// What it holds.
    pub content: Content,
    /// What couldn't be read.
    pub problems: Vec<String>,
}

/// Read a file of `kind`.
#[must_use]
pub fn read(kind: Kind, data: &[u8]) -> Read {
    let mut problems = Vec::new();
    let text = String::from_utf8_lossy(data);
    let content = match kind {
        Kind::DockerContainer => Content::Container(Box::new(match json::parse(&text) {
            Ok(document) => container(&document),
            Err(why) => {
                problems.push(format!("not JSON: {why}"));
                Container::default()
            }
        })),
        Kind::DockerLayer => Content::Layer(match json::parse(&text) {
            Ok(document) => layer(&document),
            Err(why) => {
                problems.push(format!("not JSON: {why}"));
                Layer::default()
            }
        }),
        Kind::DockerLog => Content::Log(docker_log(&text, &mut problems)),
        Kind::CriLog => Content::Log(cri_log(&text, &mut problems)),
    };
    Read { content, problems }
}

fn path<'a>(json: &'a Json, path: &str) -> Option<&'a Json> {
    path.split('.').try_fold(json, |value, key| value.get(key))
}

fn text(json: &Json, at: &str) -> Option<String> {
    path(json, at)
        .and_then(Json::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn strings(json: Option<&Json>) -> Vec<String> {
    json.and_then(Json::as_array)
        .unwrap_or_default()
        .iter()
        .filter_map(|v| v.as_str().map(str::to_owned))
        .collect()
}

/// A time, Docker's zero (`0001-01-01T00:00:00Z`) as none.
fn moment(json: &Json, at: &str) -> Option<Ts> {
    text(json, at)
        .filter(|t| !t.starts_with("0001-01-01"))
        .and_then(|t| time::parse(&t))
}

fn boolean(json: &Json, at: &str) -> Option<bool> {
    match path(json, at)? {
        Json::Bool(b) => Some(*b),
        _ => None,
    }
}

fn container(document: &Json) -> Container {
    let mut command: Vec<String> = text(document, "Path").into_iter().collect();
    command.extend(strings(document.get("Args")));
    let members = |at: &str| match path(document, at) {
        Some(Json::Object(members)) => members.as_slice(),
        _ => &[],
    };
    let mut mounts: Vec<String> = members("MountPoints")
        .iter()
        .map(|(destination, mount)| {
            let source = text(mount, "Source").unwrap_or_default();
            format!("{source}:{destination}")
        })
        .collect();
    if mounts.is_empty() {
        mounts = members("Config.Volumes")
            .iter()
            .map(|(d, _)| d.clone())
            .collect();
    }
    let ports = members("HostConfig.PortBindings")
        .iter()
        .flat_map(|(port, bindings)| {
            bindings
                .as_array()
                .unwrap_or_default()
                .iter()
                .map(move |binding| {
                    format!(
                        "{port}->{}:{}",
                        text(binding, "HostIp").unwrap_or_else(|| "0.0.0.0".to_owned()),
                        text(binding, "HostPort").unwrap_or_default()
                    )
                })
        })
        .collect();
    Container {
        id: text(document, "ID"),
        name: text(document, "Name").map(|n| n.trim_start_matches('/').to_owned()),
        hostname: text(document, "Config.Hostname"),
        image: text(document, "Config.Image"),
        image_id: text(document, "Image"),
        command: Some(command.join(" ")).filter(|c| !c.is_empty()),
        user: text(document, "Config.User"),
        env: strings(path(document, "Config.Env")),
        mounts,
        ports,
        privileged: boolean(document, "HostConfig.Privileged"),
        created: moment(document, "Created"),
        started: moment(document, "State.StartedAt"),
        finished: moment(document, "State.FinishedAt"),
        running: boolean(document, "State.Running"),
    }
}

fn layer(document: &Json) -> Layer {
    let command = strings(path(document, "container_config.Cmd"));
    Layer {
        id: text(document, "id"),
        created: moment(document, "created"),
        command: Some(command.join(" ")).filter(|c| !c.is_empty()),
        author: text(document, "author"),
    }
}

fn docker_log(text: &str, problems: &mut Vec<String>) -> Vec<LogLine> {
    let mut lines = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        match json::parse(line) {
            Ok(entry) => lines.push(LogLine {
                line: index + 1,
                time: moment(&entry, "time"),
                stream: text_or_empty(&entry, "stream"),
                text: entry
                    .get("log")
                    .and_then(Json::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                partial: false,
            }),
            Err(why) => problems.push(format!("line {}: {why}", index + 1)),
        }
    }
    lines
}

fn text_or_empty(json: &Json, at: &str) -> String {
    text(json, at).unwrap_or_default()
}

fn cri_log(text: &str, problems: &mut Vec<String>) -> Vec<LogLine> {
    let mut lines = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        match cri_line(line) {
            Some(mut entry) => {
                entry.line = index + 1;
                lines.push(entry);
            }
            None => problems.push(format!("line {}: not a CRI log line", index + 1)),
        }
    }
    lines
}

/// `<RFC 3339 time> <stdout|stderr> <P|F> <text>`.
fn cri_line(line: &str) -> Option<LogLine> {
    let mut parts = line.splitn(4, ' ');
    let time = time::parse(parts.next()?)?;
    let stream = parts.next()?;
    if stream != "stdout" && stream != "stderr" {
        return None;
    }
    let tag = parts.next()?;
    let partial = match tag.split(':').next()? {
        "P" => true,
        "F" => false,
        _ => return None,
    };
    Some(LogLine {
        line: 0,
        time: Some(time),
        stream: stream.to_owned(),
        text: parts.next().unwrap_or_default().to_owned(),
        partial,
    })
}
