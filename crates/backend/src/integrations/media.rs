//! Local media blob store for AI-generated assets.

use std::path::{Path, PathBuf};

use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct StoredMedia {
    pub id: Uuid,
    pub path: PathBuf,
    pub mime: String,
    pub ext: String,
    /// Relative API path: `/api/media/{id}.{ext}`
    pub uri: String,
}

#[derive(Clone)]
pub struct MediaStore {
    root: PathBuf,
}

impl MediaStore {
    pub fn new(data_dir: impl AsRef<Path>) -> Self {
        Self {
            root: data_dir.as_ref().join("media"),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub async fn ensure_dir(&self) -> Result<(), String> {
        tokio::fs::create_dir_all(&self.root)
            .await
            .map_err(|e| format!("media: create dir: {e}"))
    }

    pub async fn save_bytes(
        &self,
        bytes: &[u8],
        mime: &str,
        ext: &str,
    ) -> Result<StoredMedia, String> {
        self.ensure_dir().await?;
        let id = Uuid::now_v7();
        let ext = ext.trim_start_matches('.').to_ascii_lowercase();
        let filename = format!("{id}.{ext}");
        let path = self.root.join(&filename);
        tokio::fs::write(&path, bytes)
            .await
            .map_err(|e| format!("media: write: {e}"))?;
        Ok(StoredMedia {
            id,
            path,
            mime: mime.to_string(),
            ext: ext.clone(),
            uri: format!("/api/media/{id}.{ext}"),
        })
    }

    pub async fn save_base64(
        &self,
        b64: &str,
        mime: &str,
        ext: &str,
    ) -> Result<StoredMedia, String> {
        use base64::Engine as _;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64.trim())
            .map_err(|e| format!("media: base64 decode: {e}"))?;
        self.save_bytes(&bytes, mime, ext).await
    }

    /// Resolve `id` or `id.ext` to a file path under the media root.
    pub fn resolve_public(&self, name: &str) -> Option<(PathBuf, String)> {
        let name = name.trim().trim_start_matches('/');
        if name.contains("..") || name.contains('/') || name.contains('\\') {
            return None;
        }
        let path = self.root.join(name);
        if !path.is_file() {
            return None;
        }
        let mime = mime_from_name(name);
        Some((path, mime))
    }

    pub async fn read_input_bytes(&self, source: &str) -> Result<(Vec<u8>, String), String> {
        let trimmed = source.trim();
        if trimmed.is_empty() {
            return Err("media: empty input".into());
        }
        if let Some(rest) = trimmed.strip_prefix("data:") {
            // data:[mime];base64,...
            let (meta, data) = rest
                .split_once(',')
                .ok_or_else(|| "media: invalid data URI".to_string())?;
            let mime = meta
                .split(';')
                .next()
                .filter(|s| !s.is_empty())
                .unwrap_or("application/octet-stream")
                .to_string();
            use base64::Engine as _;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(data.trim())
                .map_err(|e| format!("media: data uri decode: {e}"))?;
            return Ok((bytes, mime));
        }
        if trimmed.starts_with("/api/media/") {
            let name = trimmed.trim_start_matches("/api/media/");
            let (path, mime) = self
                .resolve_public(name)
                .ok_or_else(|| format!("media: not found `{name}`"))?;
            let bytes = tokio::fs::read(&path)
                .await
                .map_err(|e| format!("media: read: {e}"))?;
            return Ok((bytes, mime));
        }
        if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
            let client = reqwest::Client::new();
            let res = client
                .get(trimmed)
                .send()
                .await
                .map_err(|e| format!("media: download: {e}"))?;
            if !res.status().is_success() {
                return Err(format!("media: download HTTP {}", res.status()));
            }
            let mime = res
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("application/octet-stream")
                .to_string();
            let bytes = res
                .bytes()
                .await
                .map_err(|e| format!("media: download body: {e}"))?
                .to_vec();
            return Ok((bytes, mime));
        }
        // Local path relative to media root or absolute
        let path = if Path::new(trimmed).is_absolute() {
            PathBuf::from(trimmed)
        } else {
            self.root.join(trimmed)
        };
        let bytes = tokio::fs::read(&path)
            .await
            .map_err(|e| format!("media: read file: {e}"))?;
        Ok((bytes, mime_from_name(path.to_string_lossy().as_ref())))
    }
}

fn mime_from_name(name: &str) -> String {
    match name.rsplit('.').next().unwrap_or("").to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "json" => "application/json",
        "txt" => "text/plain",
        _ => "application/octet-stream",
    }
    .to_string()
}
