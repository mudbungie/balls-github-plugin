use std::io;

#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("http: {0}")]
    Http(#[from] reqwest::Error),
    #[error("auth: {0}")]
    Auth(String),
    #[error("config: {0}")]
    Config(String),
    #[error("github api {status}: {body}")]
    GithubApi { status: u16, body: String },
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, PluginError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn io_error_display() {
        let e: PluginError = io::Error::new(io::ErrorKind::NotFound, "gone").into();
        assert!(e.to_string().contains("gone"));
    }

    #[test]
    fn json_error_display() {
        let e: PluginError = serde_json::from_str::<String>("bad").unwrap_err().into();
        assert!(e.to_string().contains("json"));
    }

    #[test]
    fn auth_error_display() {
        assert_eq!(PluginError::Auth("x".into()).to_string(), "auth: x");
    }

    #[test]
    fn config_error_display() {
        assert_eq!(PluginError::Config("y".into()).to_string(), "config: y");
    }

    #[test]
    fn github_api_error_display() {
        let e = PluginError::GithubApi {
            status: 404,
            body: "nope".into(),
        };
        assert_eq!(e.to_string(), "github api 404: nope");
    }

    #[test]
    fn other_error_display() {
        assert_eq!(PluginError::Other("z".into()).to_string(), "z");
    }
}
