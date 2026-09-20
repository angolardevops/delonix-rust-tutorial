use std::process::Command;

fn host_name() -> String {
    String::from_utf8_lossy(&Command::new("hostname").output().expect("hostname").stdout).trim().to_owned()
}

#[test]
fn a_new_uts_and_pid_namespace_isolate_the_child_and_leave_the_host_alone() {
    let before = host_name();
    let out = Command::new(env!("CARGO_BIN_EXE_ns_demo")).output().expect("run ns_demo");
    let text = String::from_utf8_lossy(&out.stdout);
    if String::from_utf8_lossy(&out.stderr).contains("SKIP") {
        assert!(
            std::env::var_os("MC_REQUIRE_E2E").is_none(),
            "E2E obrigatório em CI mas os user namespaces estão indisponíveis"
        );
        eprintln!("SKIP: user namespaces indisponíveis");
        return;
    }
    assert!(text.contains("child: pid=1 uid=0 hostname=dentro-do-ns"), "{text}");
    // A prova que interessa: o hostname do host, visto de FORA, não mudou.
    assert_eq!(host_name(), before);
}
