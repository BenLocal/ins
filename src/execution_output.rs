use std::sync::{Arc, Mutex};

#[derive(Clone, Debug)]
pub struct ExecutionOutput {
    inner: Arc<Mutex<String>>,
    echo: bool,
}

impl ExecutionOutput {
    pub fn stdout() -> Self {
        Self {
            inner: Arc::new(Mutex::new(String::new())),
            echo: true,
        }
    }

    pub fn buffered() -> Self {
        Self {
            inner: Arc::new(Mutex::new(String::new())),
            echo: false,
        }
    }

    pub fn line(&self, message: impl AsRef<str>) {
        let message = message.as_ref();
        if self.echo {
            println!("{message}");
        }
        let mut buffer = self.inner.lock().expect("execution output lock poisoned");
        if !buffer.is_empty() {
            buffer.push('\n');
        }
        buffer.push_str(message);
    }

    pub fn error_line(&self, message: impl AsRef<str>) {
        let message = message.as_ref();
        if self.echo {
            eprintln!("{message}");
        }
        let mut buffer = self.inner.lock().expect("execution output lock poisoned");
        if !buffer.is_empty() {
            buffer.push('\n');
        }
        buffer.push_str(message);
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
mod tests {
    use super::ExecutionOutput;

    #[test]
    fn execution_output_buffers_lines_in_order() {
        let output = ExecutionOutput::buffered();
        output.line("Starting check...");
        output.line("Check completed.");
        output.error_line("warning line");

        assert_eq!(
            output.snapshot(),
            "Starting check...\nCheck completed.\nwarning line"
        );
    }
}
