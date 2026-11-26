// The desktop tunnel must never expose the sidecar's other local APIs.
pub(super) fn allowed(method: &str, target: &str, websocket: bool) -> bool {
    let path = target.split('?').next().unwrap_or("");
    matches!(
        (method, path, websocket),
        ("GET", "/ui" | "/pty", true)
            | (
                "GET",
                "/kill"
                    | "/run/commands"
                    | "/run/status"
                    | "/run/restart"
                    | "/run/start"
                    | "/run/stop",
                false
            )
            | ("POST", "/agent/send", false)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_routine_hook_is_never_reachable_through_the_tunnel() {
        assert!(!allowed("POST", "/agents/routine-hook/rt123456", false));
        assert!(!allowed("POST", "/agents/routine-create", false));
    }

    #[test]
    fn allows_only_frontend_routes_and_their_transports() {
        for path in ["/ui", "/pty?tileId=x"] {
            assert!(allowed("GET", path, true));
            assert!(!allowed("GET", path, false));
        }
        for path in [
            "/kill",
            "/run/commands",
            "/run/status",
            "/run/restart",
            "/run/start",
            "/run/stop",
        ] {
            assert!(allowed("GET", path, false));
            assert!(!allowed("POST", path, false));
        }
        assert!(allowed("POST", "/agent/send?tileId=x", false));
        for path in [
            "/mobile/state",
            "/mobile/pair",
            "/run/logs",
            "/capture",
            "/pty/../mobile/state",
            "http://localhost/pty",
        ] {
            assert!(!allowed("GET", path, false));
            assert!(!allowed("GET", path, true));
        }
    }
}
