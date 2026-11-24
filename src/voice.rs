//! Local microphone capture and offline Whisper transcription.
use std::path::PathBuf;
use std::process::{Child, Command};
#[cfg(target_os = "linux")]
use std::process::Stdio;

pub struct Recording {
    child: Option<Child>,
    path: PathBuf,
}

impl Recording {
    pub fn start() -> Result<Self, String> {
        // Capture shells out to ALSA's `arecord` and transcription to a local
        // faster-whisper install: Linux only. Elsewhere the mic button reports
        // this instead of failing obscurely.
        #[cfg(not(target_os = "linux"))]
        return Err("Voice input is only available on Linux for now.".into());
        #[cfg(target_os = "linux")]
        Self::start_linux()
    }

    #[cfg(target_os = "linux")]
    fn start_linux() -> Result<Self, String> {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "joebro-voice-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir(&dir).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
                .map_err(|e| e.to_string())?;
        }
        let path = dir.join("recording.wav");
        let child = Command::new("arecord")
            .args(["-q", "-f", "S16_LE", "-r", "16000", "-c", "1", "-d", "300"])
            .arg(&path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                let _ = std::fs::remove_dir_all(&dir);
                format!("Microphone capture unavailable: {e}")
            })?;
        Ok(Self {
            child: Some(child),
            path,
        })
    }

    pub fn finished(&mut self) -> bool {
        self.child
            .as_mut()
            .and_then(|c| c.try_wait().ok())
            .flatten()
            .is_some()
    }

    pub fn transcribe(mut self) -> Result<String, String> {
        if let Some(child) = self.child.take() {
            // SIGINT lets arecord finalise the WAV header.
            #[cfg(unix)]
            let _ = Command::new("kill")
                .args(["-INT", &child.id().to_string()])
                .status();
            #[cfg(not(unix))]
            let mut child = child;
            #[cfg(not(unix))]
            let _ = child.kill();
            let output = child.wait_with_output().map_err(|e| e.to_string())?;
            if !self.path.exists()
                || std::fs::metadata(&self.path)
                    .map(|m| m.len() < 1000)
                    .unwrap_or(true)
            {
                return Err(format!(
                    "No microphone audio captured. {}",
                    String::from_utf8_lossy(&output.stderr)
                ));
            }
        }
        let python = std::env::var("JOEBRO_VOICE_PYTHON").unwrap_or_else(|_| "python3".into());
        let script = r#"
import json, os, pathlib, sys
from faster_whisper import WhisperModel
base = pathlib.Path.home()/'.cache/huggingface/hub/models--Systran--faster-whisper-small/snapshots'
cached = sorted(base.glob('*'))
model_path = os.environ.get('JOEBRO_VOICE_MODEL') or (str(cached[0]) if cached else 'small')
model = WhisperModel(model_path, device='cpu', compute_type='int8', local_files_only=True)
segments, _ = model.transcribe(sys.argv[1], beam_size=3, vad_filter=True)
print(json.dumps({'text': ' '.join(s.text.strip() for s in segments)}))
"#;
        let out = Command::new(python)
            .args(["-c", script])
            .arg(&self.path)
            .output()
            .map_err(|e| format!("Voice transcription unavailable: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "Local Whisper transcription failed: {}",
                String::from_utf8_lossy(&out.stderr)
                    .chars()
                    .rev()
                    .take(700)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect::<String>()
            ));
        }
        let value: serde_json::Value =
            serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?;
        let text = value["text"].as_str().unwrap_or("").trim().to_string();
        if text.is_empty() {
            Err("No speech detected. Try recording again.".into())
        } else {
            Ok(text)
        }
    }
}

impl Drop for Recording {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(dir) = self.path.parent() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}
