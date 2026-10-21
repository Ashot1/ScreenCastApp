import Darwin

/// Process-wide setup that the app runs first at launch.
public enum AmbientProcess {
    /// Local plugin servers are child processes of this app: a write to one that already exited
    /// must fail with EPIPE inside Rust (→ «unavailable»), not kill the app with SIGPIPE. Rust
    /// ignores SIGPIPE only in its own executables, and std restores the default in children.
    public static func launch() {
        _ = signal(SIGPIPE, SIG_IGN)
    }
}
