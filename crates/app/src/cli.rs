use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

use syncplay_core::config::SyncplayConfig;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct LaunchOptions {
    pub host: Option<String>,
    pub port: Option<u16>,
    pub username: Option<String>,
    pub room: Option<String>,
    pub password: Option<String>,
    pub player_path: Option<String>,
    pub player_args: Vec<String>,
    pub media: Option<String>,
    pub language: Option<String>,
    pub no_store: bool,
    pub debug: bool,
    pub external: bool,
}

impl LaunchOptions {
    pub fn apply(&self, config: &mut SyncplayConfig) {
        if let Some(host) = &self.host {
            let (host, port) = split_host(host, self.port.unwrap_or(config.server.port));
            config.server.host = host;
            config.server.port = port;
        } else if let Some(port) = self.port {
            config.server.port = port;
        }
        if let Some(username) = &self.username {
            config.user.username = username.clone();
        }
        if let Some(room) = &self.room {
            config.user.default_room = room.clone();
        }
        if self.password.is_some() {
            config.server.password = self.password.clone();
        }
        if let Some(path) = &self.player_path {
            config.player.player_path = path.clone();
        }
        config
            .player
            .player_arguments
            .extend(self.player_args.iter().cloned());
        if let Some(language) = &self.language {
            config.user.language = language.clone();
        }
        if self.debug {
            config.user.debug = true;
        }
    }

    pub fn no_store(&self) -> bool {
        self.no_store || self.external
    }

    pub fn should_autoconnect(&self) -> bool {
        self.external
    }
}

pub enum ParseOutcome {
    Launch(LaunchOptions),
    Exit(ExitCode),
}

pub fn parse_env() -> Result<ParseOutcome, String> {
    parse(std::env::args_os().skip(1))
}

pub fn help() -> &'static str {
    "Syncplay - synchronize video playback\n\n\
Usage: syncplay [OPTIONS] [FILE] [-- PLAYER_ARGS...]\n\n\
Options:\n\
  -a, --host HOST[:PORT]    Server address\n\
  -n, --name USERNAME       Username\n\
  -r, --room ROOM           Room to join\n\
  -p, --password PASSWORD   Server password\n\
      --player-path PATH    Media player executable\n\
      --language LANGUAGE   UI language\n\
      --no-store            Do not save configuration changes\n\
      --no-gui              Accepted for compatibility; the GUI is still shown\n\
  -d, --debug               Enable debug messages\n\
  -g, --force-gui-prompt    Accepted for compatibility\n\
  -v, --version             Print version\n\
  -h, --help                Print help\n\n\
Everything after -- is passed to the media player. Supplying a media file\n\
automatically connects using the saved settings. Launch-specific options and\n\
player arguments are not written to the saved configuration."
}

fn parse(args: impl IntoIterator<Item = OsString>) -> Result<ParseOutcome, String> {
    let mut options = LaunchOptions::default();
    let mut args = args.into_iter();
    let mut positional_only = false;
    while let Some(raw) = args.next() {
        let value = raw
            .into_string()
            .map_err(|_| "Arguments must be valid UTF-8".to_string())?;
        if positional_only {
            options.player_args.push(value);
            continue;
        }
        match value.as_str() {
            "--" => positional_only = true,
            "-h" | "--help" => {
                println!("{}", help());
                return Ok(ParseOutcome::Exit(ExitCode::SUCCESS));
            }
            "-v" | "--version" => {
                println!("Syncplay {}", env!("CARGO_PKG_VERSION"));
                return Ok(ParseOutcome::Exit(ExitCode::SUCCESS));
            }
            "--no-store" => options.no_store = true,
            "--no-gui" | "-g" | "--force-gui-prompt" => {}
            "-d" | "--debug" => options.debug = true,
            "-a" | "--host" => options.host = Some(next_value(&mut args, &value)?),
            "-n" | "--name" => options.username = Some(next_value(&mut args, &value)?),
            "-r" | "--room" => options.room = Some(next_value(&mut args, &value)?),
            "-p" | "--password" => options.password = nonempty(next_value(&mut args, &value)?),
            "--player-path" => options.player_path = Some(next_value(&mut args, &value)?),
            "--language" => options.language = Some(next_value(&mut args, &value)?),
            _ if value.starts_with("-psn") => {}
            _ if value.starts_with("--host=") => options.host = Some(value[7..].to_string()),
            _ if value.starts_with("--name=") => options.username = Some(value[7..].to_string()),
            _ if value.starts_with("--room=") => options.room = Some(value[7..].to_string()),
            _ if value.starts_with("--password=") => {
                options.password = nonempty(value[11..].to_string())
            }
            _ if value.starts_with("--player-path=") => {
                options.player_path = Some(value[14..].to_string())
            }
            _ if value.starts_with("--language=") => {
                options.language = Some(value[11..].to_string())
            }
            _ if value.starts_with('-') => {
                return Err(format!("Unknown option: {value}"));
            }
            _ if options.media.is_none() => {
                options.media = Some(normalize_media(value));
                options.external = true;
            }
            _ => options.player_args.push(value),
        }
    }
    Ok(ParseOutcome::Launch(options))
}

fn next_value(args: &mut impl Iterator<Item = OsString>, option: &str) -> Result<String, String> {
    args.next()
        .ok_or_else(|| format!("Missing value for {option}"))?
        .into_string()
        .map_err(|_| format!("Value for {option} must be valid UTF-8"))
}

fn nonempty(value: String) -> Option<String> {
    (!value.is_empty()).then_some(value)
}

fn normalize_media(value: String) -> String {
    if value.contains("://") || PathBuf::from(&value).is_absolute() {
        return value;
    }
    std::path::absolute(&value)
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or(value)
}

fn split_host(value: &str, default_port: u16) -> (String, u16) {
    if let Some(host) = value.strip_prefix('[') {
        if let Some((host, port)) = host.split_once("]:") {
            return (host.to_string(), parse_port(port, default_port));
        }
        return (host.trim_end_matches(']').to_string(), default_port);
    }
    if value.matches(':').count() == 1 {
        if let Some((host, port)) = value.rsplit_once(':') {
            return (host.to_string(), parse_port(port, default_port));
        }
    }
    (value.to_string(), default_port)
}

fn parse_port(value: &str, default_port: u16) -> u16 {
    value.parse().unwrap_or(default_port)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(args: &[&str]) -> LaunchOptions {
        match parse(args.iter().map(OsString::from)).unwrap() {
            ParseOutcome::Launch(options) => options,
            ParseOutcome::Exit(_) => panic!("unexpected early exit"),
        }
    }

    #[test]
    fn parses_ani_cli_invocation_without_losing_player_arguments() {
        let parsed = options(&[
            "https://media.example/episode.m3u8",
            "--",
            "--referrer=https://www.example.com/",
            "--audio-file=https://media.example/audio.m3u8",
            "--sub-file=https://media.example/subtitles.ass",
            "--force-media-title=Example Episode 1",
        ]);

        assert_eq!(
            parsed.media.as_deref(),
            Some("https://media.example/episode.m3u8")
        );
        assert_eq!(
            parsed.player_args,
            [
                "--referrer=https://www.example.com/",
                "--audio-file=https://media.example/audio.m3u8",
                "--sub-file=https://media.example/subtitles.ass",
                "--force-media-title=Example Episode 1",
            ]
        );
        assert!(parsed.should_autoconnect());
        assert!(parsed.no_store());
    }

    #[test]
    fn applies_connection_overrides_without_adding_player_args_to_saved_config() {
        let parsed = options(&[
            "-a",
            "sync.example:9000",
            "-n",
            "alice",
            "-r",
            "watch",
            "-p",
            "secret",
            "--player-path",
            "/opt/mpv",
            "video.mkv",
            "--",
            "--fullscreen",
        ]);
        let mut config = SyncplayConfig::default();
        config.player.player_arguments = vec!["--existing".into()];
        parsed.apply(&mut config);

        assert_eq!(config.server.host, "sync.example");
        assert_eq!(config.server.port, 9000);
        assert_eq!(config.server.password.as_deref(), Some("secret"));
        assert_eq!(config.user.username, "alice");
        assert_eq!(config.user.default_room, "watch");
        assert_eq!(config.player.player_path, "/opt/mpv");
        assert_eq!(
            config.player.player_arguments,
            ["--existing", "--fullscreen"]
        );
    }

    #[test]
    fn recognizes_ipv6_and_macos_finder_arguments() {
        let parsed = options(&["-psn_0_12345", "-a", "[::1]:8998"]);
        let mut config = SyncplayConfig::default();
        parsed.apply(&mut config);
        assert_eq!(config.server.host, "::1");
        assert_eq!(config.server.port, 8998);
        assert!(!parsed.should_autoconnect());
    }

    #[test]
    fn rejects_unknown_options_instead_of_starting_unexpectedly() {
        assert!(parse([OsString::from("--unknown")]).is_err());
        assert!(parse([OsString::from("--host")]).is_err());
    }
}
