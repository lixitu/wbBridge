//! 运行期红线守卫。
//!
//! 把 AGENTS.md「安全红线」「供应链红线」里能静态断言的条款钉成 `cargo test` 会跑的测试：
//! 一旦有人改动鉴权、Origin 拦截、权限表、隔离配置、并发/体积上限、子进程环境白名单
//! 或运行时下载来源白名单，这里会直接失败并指出该条款来自哪里。行为级红线（探测不撤销发布、
//! 客户端取消不记成功、错误码映射等）已在各模块 `#[cfg(test)]` 单元用例中覆盖，本文件不重复。
//!
//! 另有一条无法由测试覆盖，靠代码审查守住：**监听地址只能是回环**
//! （`src-tauri/core/src/orchestration.rs` 里 `TcpListener::bind(("127.0.0.1", port))`）。

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

use wbbridge_core::{backend, platform, probe, runtime, server, sync};

const KEY: &str = "red-line-test-key";

/// 全部对外/管理路由：每一条都必须过 Bearer 鉴权（含 `/health`，历史上曾漏过一次）。
///
/// `POST /admin/*` 直接取自 `server::ACTION_ROUTES`（唯一真相），不再手写第二份清单：
/// 手写表会在有人新增动作时悄悄漏掉那一条，红线测试于是只守住了旧的 5 条。
fn routes() -> Vec<(&'static str, Method)> {
    let mut listed = vec![
        ("/health", Method::GET),
        ("/v1/models", Method::GET),
        ("/v1/chat/completions", Method::POST),
    ];
    listed.extend(server::ACTION_ROUTES.iter().map(|(_, method, path)| {
        (
            *path,
            Method::from_bytes(method.as_bytes()).expect("动作表里的方法必须是合法 HTTP 方法"),
        )
    }));
    listed
}

fn router() -> axum::Router {
    server::Server::new(KEY).build().0
}

fn request(method: Method, path: &str, authorization: Option<&str>, origin: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(value) = authorization {
        builder = builder.header(header::AUTHORIZATION, value);
    }
    if let Some(value) = origin {
        builder = builder.header(header::ORIGIN, value);
    }
    builder
        .body(Body::from(json!({}).to_string()))
        .expect("请求可构造")
}

#[tokio::test]
async fn every_route_requires_bearer_authorization() {
    for (path, method) in routes() {
        let label = format!("{method} {path}");

        // 无 Authorization → 401
        let status = router()
            .oneshot(request(method.clone(), path, None, None))
            .await
            .expect("路由可响应")
            .status();
        assert_eq!(
            status,
            StatusCode::UNAUTHORIZED,
            "{label} 缺少鉴权时必须 401（红线：所有路由含 /health 都要 Bearer）"
        );

        // 错误 key → 401
        let status = router()
            .oneshot(request(method.clone(), path, Some("Bearer not-the-key"), None))
            .await
            .expect("路由可响应")
            .status();
        assert_eq!(
            status,
            StatusCode::UNAUTHORIZED,
            "{label} 携带错误 key 时必须 401"
        );

        // 裸 key（非 Bearer 方案）→ 401：鉴权只接受 `Authorization: Bearer <api-key>`
        let status = router()
            .oneshot(request(method.clone(), path, Some(KEY), None))
            .await
            .expect("路由可响应")
            .status();
        assert_eq!(
            status,
            StatusCode::UNAUTHORIZED,
            "{label} 必须只接受 Bearer 方案，裸 key 不得放行"
        );

        // 正确 Bearer → 不再被鉴权拦下
        let status = router()
            .oneshot(request(method, path, Some(&format!("Bearer {KEY}")), None))
            .await
            .expect("路由可响应")
            .status();
        assert_ne!(
            status,
            StatusCode::UNAUTHORIZED,
            "{label} 带正确 key 时不应返回 401"
        );
    }
}

#[tokio::test]
async fn any_non_empty_origin_is_rejected_with_403() {
    for (path, method) in routes() {
        let label = format!("{method} {path}");
        for origin in ["https://evil.example.com", "null", "http://localhost:5173"] {
            let status = router()
                .oneshot(request(
                    method.clone(),
                    path,
                    Some(&format!("Bearer {KEY}")),
                    Some(origin),
                ))
                .await
                .expect("路由可响应")
                .status();
            assert_eq!(
                status,
                StatusCode::FORBIDDEN,
                "{label} 带 Origin: {origin} 必须 403（红线：拒绝一切浏览器来源）"
            );
        }
    }
}

/// 红线 1 的另一半：**空密钥不构成可用的鉴权方案**。
///
/// `authorized` 把期望值拼成 `format!("Bearer {key}")`，key 为空时它就是 `"Bearer "`。
/// 只要 api-key 文件被截断成空白又被引导沿用，任何本机进程带着尾随空格的头都能通过全部路由。
/// 引导侧已有 `orchestration::resolve_api_key` 保证非空，这里钉死服务端这道边界本身。
#[tokio::test]
async fn an_empty_api_key_authorizes_nothing() {
    let app = server::Server::new(String::new()).build().0;
    for (path, method) in routes() {
        let label = format!("{method} {path}");
        for header in ["Bearer ", "Bearer", "Bearer x"] {
            let status = app
                .clone()
                .oneshot(request(method.clone(), path, Some(header), None))
                .await
                .expect("路由可响应")
                .status();
            assert_eq!(
                status,
                StatusCode::UNAUTHORIZED,
                "{label} 在空密钥下必须 401（尝试头 {header:?}）"
            );
        }
    }
}

#[test]
fn transport_limits_are_not_loosened() {
    assert_eq!(server::MAX_BODY_BYTES, 8 * 1024 * 1024, "请求体上限固定 8MB，超出应 413");
    assert_eq!(server::MAX_CONCURRENT_REQUESTS, 4, "并发上限固定 4，超出应 429 busy");
    assert_eq!(
        probe::PROBE_TIMEOUT_MS,
        60_000,
        "每个模型各自 60s 探测预算（重试共用同一 deadline），不得放宽"
    );
}

#[test]
fn native_tools_stay_ask_or_deny_and_never_allow() {
    let permissions = backend::native_permissions();
    assert_eq!(
        permissions.get("*").and_then(Value::as_str),
        Some("ask"),
        "默认权限必须是 ask"
    );
    for name in [
        "question",
        "websearch",
        "codesearch",
        "webfetch",
        "task",
        "plan_enter",
        "plan_exit",
        "todowrite",
    ] {
        assert_eq!(
            permissions.get(name).and_then(Value::as_str),
            Some("deny"),
            "{name} 必须保持 deny：模型绝不允许自行执行本地动作"
        );
    }
    for (name, action) in permissions.as_object().expect("权限是对象") {
        assert_ne!(
            action.as_str(),
            Some("allow"),
            "{name} 被放开成 allow：原生工具只能以 handoff 交回客户端"
        );
    }
}

#[test]
fn isolated_runtime_config_keeps_autoupdate_off_and_agents_present() {
    let config = runtime::isolated_config();
    assert_eq!(
        config.get("autoupdate"),
        Some(&json!(false)),
        "autoupdate 必须为 false：托管运行时不得自更新"
    );
    assert_eq!(
        config.get("share"),
        Some(&json!("disabled")),
        "share 必须 disabled：隔离会话不得上报"
    );
    assert_eq!(
        config.get("permission"),
        Some(&backend::native_permissions()),
        "子进程权限表必须与 native_permissions 完全一致"
    );
    for agent in ["buddy-bridge", "buddy-chat"] {
        assert!(
            config
                .pointer(&format!("/agent/{agent}"))
                .and_then(Value::as_object)
                .is_some(),
            "缺少自定义 agent {agent}：/agent 校验会挡住 ready，不得删除"
        );
    }
}

#[test]
fn subprocess_environment_only_passes_the_allow_list() {
    // 白名单本身不得含凭据类变量名（否则等于把其他 provider 的 Key 透传进子进程）。
    for name in runtime::ENV_ALLOW {
        let upper = name.to_ascii_uppercase();
        assert!(
            !upper.contains("KEY") && !upper.contains("TOKEN") && !upper.contains("PASSWORD"),
            "环境白名单出现凭据类变量 {name}"
        );
    }
    let mut host = HashMap::new();
    for name in [
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "OPENCODE_SERVER_PASSWORD",
        "HOME",
        "PATH",
    ] {
        host.insert(name.to_string(), format!("leak-{name}"));
    }
    let env = runtime::isolated_environment(
        "/tmp/wbbridge-red-line",
        &HashMap::new(),
        "generated-password",
        &host,
    );
    assert!(!env.contains_key("OPENAI_API_KEY"), "宿主 provider Key 泄漏进子进程");
    assert!(!env.contains_key("ANTHROPIC_API_KEY"), "宿主 provider Key 泄漏进子进程");
    assert_eq!(
        env.get("OPENCODE_SERVER_PASSWORD").map(String::as_str),
        Some("generated-password"),
        "子进程密码只能用核心生成的那一个，而不是宿主的"
    );
}

#[test]
fn sync_only_owns_entries_tagged_with_its_marker() {
    assert_eq!(
        sync::OWNER,
        "buddy-bridge-v1",
        "同步归属标记变了：会误删/误改用户手写的 models.json 条目"
    );
}

/// 供应链红线：元数据与 tarball 只能来自白名单 registry。
///
/// 这里走端到端而不是单测私有函数：被篡改的元数据可以把 `dist.tarball` 指向站外、
/// 同时保留 `/包名/-/` 的形状，而 `dist.integrity` 与它同源，所以 sha512 校验救不了这一层。
/// 断言"站外主机一次请求都没发出"才能证明下载器没被元数据牵着走。
#[tokio::test]
async fn runtime_downloads_never_leave_the_registry_allow_list() {
    let pkg = platform::runtime_package(platform::host_platform(), platform::host_arch())
        .expect("当前平台必须有托管运行时包名");
    let evil = format!("https://evil.example.com/{}/-/{}-9.9.9.tgz", pkg.name, pkg.name);
    let data_dir = std::env::temp_dir().join(format!("wbbridge-redline-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data_dir);

    let poisoned = json!({
        "name": pkg.name,
        "version": "9.9.9",
        // 形状合规的 integrity：只有 origin 越界，逼校验落在来源而不是路径上。
        "dist": { "integrity": "sha512-abc", "tarball": evil },
    })
    .to_string();
    let seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let recorder = seen.clone();
    let body = poisoned;
    let fetch = Arc::new(move |url: String, _args: runtime::FetchArgs| -> server::BoxFuture<Result<runtime::FetchReply, String>> {
        recorder.lock().unwrap().push(url.clone());
        let body = body.clone();
        Box::pin(async move {
            if url.ends_with("/latest") {
                return Ok(runtime::FetchReply {
                    status: 200,
                    body: body.into_bytes(),
                });
            }
            Err(format!("越界下载请求：{url}"))
        })
    });
    let options = runtime::RuntimeOptions {
        probe: Some(Arc::new(|_| Box::pin(async { Err("synthetic".to_string()) }))),
        fetch: Some(fetch),
        // 空候选：不得复用本机已有 opencode，否则这条红线根本走不到下载分支。
        candidates: Some(vec![]),
        ..runtime::RuntimeOptions::default()
    };

    let outcome = runtime::find_runtime(&data_dir.to_string_lossy(), Arc::new(|_| {}), options).await;
    let _ = std::fs::remove_dir_all(&data_dir);

    assert!(outcome.is_err(), "不可信的安装信息必须失败，而不是继续安装：{outcome:?}");
    let attempted = seen.lock().unwrap().clone();
    assert!(
        !attempted.iter().any(|url| url.starts_with("https://evil.example.com")),
        "站外 tarball 被请求：{attempted:?}"
    );
    assert!(
        attempted.iter().all(|url| url.ends_with("/latest")),
        "元数据判为不可信后不应再发起任何下载：{attempted:?}"
    );
}
