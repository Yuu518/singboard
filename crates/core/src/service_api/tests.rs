use super::*;
use crate::settings::Endpoint;
use std::collections::BTreeMap;

#[test]
fn editing_a_clash_backend_preserves_its_service_api_and_switching_cancels_subscriptions() {
    crate::runtime().block_on(async {
        let dir = tempfile::tempdir().unwrap();
        let a = Endpoint {
            id: "a".into(),
            url: "http://localhost:9090".into(),
            ..Default::default()
        };
        let b = Endpoint {
            id: "b".into(),
            url: "https://router.test".into(),
            ..Default::default()
        };
        Settings::edit(dir.path(), |s| {
            s.clash_apis = vec![a, b];
            s.active_clash_api_id = "a".into();
            Ok(())
        })
        .unwrap();
        let mut api = ServiceApi::new(dir.path().into());
        let conn = Connection {
            url: "https://rpc.test:8443".into(),
            secret: "service-secret".into(),
            ..Default::default()
        };
        api.execute(Command::Configure {
            backend_id: "a".into(),
            connection: Some(conn.clone()),
        })
        .await
        .unwrap();
        let binding = Binding::active(dir.path()).unwrap();
        let fields = BTreeMap::from([
            ("id".into(), "a".into()),
            ("url".into(), "https://new-clash.test".into()),
            ("secret".into(), "clash-secret".into()),
            ("name".into(), "Updated".into()),
        ]);
        crate::tools::run(dir.path(), "save_api", &fields)
            .await
            .unwrap();
        assert!(!binding.cancel.is_cancelled());
        assert!(
            Settings::load(dir.path())
                .unwrap()
                .active()
                .unwrap()
                .service_api
                .as_ref()
                == Some(&conn)
        );
        assert_eq!(
            api.execute(Command::Discover).await.unwrap(),
            json!([{"serviceTag":"","url":"https://rpc.test:8443"}])
        );
        Settings::select_endpoint(dir.path(), "b").unwrap();
        assert!(binding.cancel.is_cancelled());
        assert_eq!(
            api.execute(Command::Connect { service_tag: None })
                .await
                .unwrap_err()
                .code,
            "notConfigured"
        );
    });
}

#[test]
fn local_discovery_distinguishes_multiple_listeners_and_leaves_remote_backends_unbound() {
    crate::runtime().block_on(async {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, r#"{"services":[{"type":"api","tag":"v4","listen":"0.0.0.0","listen_port":1234,"secret":"hidden"},{"type":"api","tag":"v6","listen":"::","listen_port":443,"tls":{"enabled":true}},],}"#).unwrap();
        Settings::edit(dir.path(), |s| {
            s.config_path = path.to_str().unwrap().to_owned();
            s.native_root_enabled = true;
            s.clash_apis = vec![Endpoint {id:"local".into(),url:"http://127.0.0.1:9090".into(), ..Default::default()},
                Endpoint{id:"remote".into(),url:"https://remote.test".into(), ..Default::default()}];
            Ok(())
        }).unwrap();
        let mut api = ServiceApi::new(dir.path().into());
        assert_eq!(api.execute(Command::Discover).await.unwrap(), json!([
            {"serviceTag":"v4","url":"http://127.0.0.1:1234"},
            {"serviceTag":"v6","url":"https://[::1]:443"}
        ]));
        assert_eq!(api.execute(Command::Connect {service_tag:None}).await.unwrap_err().code, "selectService");
        Settings::select_endpoint(dir.path(), "remote").unwrap();
        assert_eq!(api.execute(Command::Discover).await.unwrap(),json!([]));
    });
}

#[test]
fn a_broken_transport_is_retryable_and_a_server_error_is_preserved() {
    let error = Status::from_error(Box::new(std::io::Error::other("closed")));
    assert_eq!(ApiError::from(error).code, "unavailable");
    assert_eq!(
        ApiError::from(Status::unknown("backend error")).code,
        "rpcError"
    );
    assert_eq!(
        ApiError::from(Status::unimplemented("old core")).code,
        "unsupported"
    );
}
