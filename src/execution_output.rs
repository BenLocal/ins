use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

#[derive(Clone, Debug)]
pub struct ExecutionOutput {
    inner: Arc<Mutex<String>>,
    echo: bool,
    tx: Option<broadcast::Sender<String>>,
}

impl ExecutionOutput {
    pub fn stdout() -> Self {
        Self {
            inner: Arc::new(Mutex::new(String::new())),
            echo: true,
            tx: None,
        }
    }

    pub fn buffered() -> Self {
        Self {
            inner: Arc::new(Mutex::new(String::new())),
            echo: false,
            tx: None,
        }
    }

    /// Creates an output that keeps the snapshot buffer **and** broadcasts each
    /// line to subscribers. Call [`subscribe`](Self::subscribe) before the job
    /// starts; late subscribers miss already-sent lines but can replay via
    /// [`snapshot`](Self::snapshot). Channel capacity is 1024.
    #[allow(dead_code)]
    pub fn streaming() -> Self {
        let (tx, _) = broadcast::channel(1024);
        Self {
            inner: Arc::new(Mutex::new(String::new())),
            echo: false,
            tx: Some(tx),
        }
    }

    /// Returns a new broadcast receiver, or `None` for non-streaming outputs
    /// (`buffered()` / `stdout()`). Receivers created after lines have already
    /// been emitted will not see them — use [`snapshot`](Self::snapshot) for
    /// catch-up.
    #[allow(dead_code)]
    pub fn subscribe(&self) -> Option<broadcast::Receiver<String>> {
        self.tx.as_ref().map(|t| t.subscribe())
    }

    pub fn line(&self, message: impl AsRef<str>) {
        let message = message.as_ref();
        if self.echo {
            println!("{message}");
        }
        {
            let mut buffer = self.inner.lock().expect("execution output lock poisoned");
            if !buffer.is_empty() {
                buffer.push('\n');
            }
            buffer.push_str(message);
        }
        if let Some(tx) = &self.tx {
            let _ = tx.send(message.to_string());
        }
    }

    pub fn error_line(&self, message: impl AsRef<str>) {
        let message = message.as_ref();
        if self.echo {
            eprintln!("{message}");
        }
        {
            let mut buffer = self.inner.lock().expect("execution output lock poisoned");
            if !buffer.is_empty() {
                buffer.push('\n');
            }
            buffer.push_str(message);
        }
        if let Some(tx) = &self.tx {
            let _ = tx.send(message.to_string());
        }
    }

    pub fn snapshot(&self) -> String {
        self.inner
            .lock()
            .expect("execution output lock poisoned")
            .clone()
    }

    pub fn echo_enabled(&self) -> bool {
        self.echo
    }
}

#[cfg(test)]
#[path = "execution_output_test.rs"]
mod execution_output_test;
