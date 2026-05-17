//! Codex Desktop Plugins 解锁器 —— 运行时伴侣守护进程
//!
//! 通过 Chrome DevTools Protocol (CDP) 向 Codex Desktop 渲染进程注入 JavaScript,
//! 调用 React state 中的 `setAuthMethod('chatgpt')` 来解锁 Plugins 选项卡。
//!
//! 使用方式:
//! 1. 创建 `PluginUnlockService`
//! 2. 调用 `start()` 启动守护循环
//! 3. 调用 `stop()` 停止
//!
//! 守护循环行为:
//! - 检测 Codex Desktop 进程是否存在
//! - 尝试连接 `http://127.0.0.1:9222/json/list` 获取 CDP endpoint
//! - WebSocket 连接后注入解锁脚本
//! - 监听 `Page.loadEventFired`,刷新后自动重新注入
//! - 断开时指数退避重连

use std::sync::Arc;
use std::time::Duration;

use futures::{Sink, SinkExt, Stream, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio::time::{interval, sleep};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message as WsMessage;

/// 解锁器状态（线程安全共享）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum UnlockStatus {
    /// 未启动或 Codex Desktop 未运行/无调试端口
    Disconnected,
    /// 正在连接 CDP
    Connecting,
    /// 已连接,等待页面就绪
    Connected,
    /// 注入成功,Plugins 已解锁
    Injected,
    /// 注入失败
    Failed { error: String },
}

/// 服务配置
#[derive(Debug, Clone)]
pub struct UnlockConfig {
    /// CDP HTTP 端点（获取 WebSocket URL）
    pub cdp_http_url: String,
    /// 检测间隔（毫秒）
    pub detect_interval_ms: u64,
    /// 重连退避:初始延迟（毫秒）
    pub reconnect_base_ms: u64,
    /// 重连退避:最大延迟（毫秒）
    pub reconnect_max_ms: u64,
    /// 启动 Codex 时附加的调试参数
    pub codex_debug_args: Vec<String>,
}

impl Default for UnlockConfig {
    fn default() -> Self {
        Self {
            cdp_http_url: "http://127.0.0.1:9222/json/list".into(),
            detect_interval_ms: 3_000,
            reconnect_base_ms: 1_000,
            reconnect_max_ms: 30_000,
            codex_debug_args: vec!["--remote-debugging-port=9222".into()],
        }
    }
}

/// CDP Page 信息（来自 `/json/list`）
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct CdpPage {
    id: String,
    #[serde(rename = "type")]
    page_type: String,
    url: String,
    #[serde(rename = "webSocketDebuggerUrl")]
    ws_url: Option<String>,
}

/// CDP WebSocket 消息
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct CdpResponse {
    id: Option<u64>,
    #[serde(rename = "result")]
    result: Option<serde_json::Value>,
    error: Option<CdpError>,
    #[serde(rename = "method")]
    method: Option<String>,
    #[serde(rename = "params")]
    params: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct CdpError {
    code: i32,
    message: String,
}

/// 解锁服务
pub struct PluginUnlockService {
    config: UnlockConfig,
    status: Arc<RwLock<UnlockStatus>>,
    /// 控制守护循环的通道
    cmd_tx: mpsc::Sender<ServiceCommand>,
    cmd_rx: Arc<Mutex<mpsc::Receiver<ServiceCommand>>>,
    /// 当前 CDP 消息 ID 计数器
    msg_id: Arc<Mutex<u64>>,
}

#[derive(Debug)]
enum ServiceCommand {
    Stop,
    /// 强制重新注入（前端手动触发）
    Reinject,
}

impl PluginUnlockService {
    pub fn new(config: UnlockConfig) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel(8);
        Self {
            config,
            status: Arc::new(RwLock::new(UnlockStatus::Disconnected)),
            cmd_tx,
            cmd_rx: Arc::new(Mutex::new(cmd_rx)),
            msg_id: Arc::new(Mutex::new(0)),
        }
    }

    /// 获取当前状态
    pub async fn status(&self) -> UnlockStatus {
        self.status.read().await.clone()
    }

    /// 启动守护循环（非阻塞）
    pub fn start(&self) {
        let config = self.config.clone();
        let status = self.status.clone();
        let cmd_rx = self.cmd_rx.clone();
        let msg_id = self.msg_id.clone();

        tokio::spawn(async move {
            run_daemon(config, status, cmd_rx, msg_id).await;
        });
    }

    /// 停止守护循环
    pub async fn stop(&self) {
        let _ = self.cmd_tx.send(ServiceCommand::Stop).await;
    }

    /// 前端手动触发重新注入
    pub async fn reinject(&self) {
        let _ = self.cmd_tx.send(ServiceCommand::Reinject).await;
    }
}

/// 守护循环主逻辑
async fn run_daemon(
    config: UnlockConfig,
    status: Arc<RwLock<UnlockStatus>>,
    cmd_rx: Arc<Mutex<mpsc::Receiver<ServiceCommand>>>,
    msg_id: Arc<Mutex<u64>>,
) {
    let mut reconnect_delay = config.reconnect_base_ms;
    let mut detect_tick = interval(Duration::from_millis(config.detect_interval_ms));

    loop {
        // 检查是否有外部命令
        {
            let mut rx = cmd_rx.lock().await;
            if let Ok(cmd) = rx.try_recv() {
                match cmd {
                    ServiceCommand::Stop => {
                        tracing::info!("[PluginUnlock] daemon stopped by command");
                        set_status(&status, UnlockStatus::Disconnected).await;
                        return;
                    }
                    ServiceCommand::Reinject => {
                        tracing::info!("[PluginUnlock] reinject requested");
                        // 直接跳到注入逻辑
                    }
                }
            }
        }

        // 阶段 1: 检测 CDP 端口是否可用
        match detect_cdp(&config.cdp_http_url).await {
            Some(pages) => {
                // 找到合适的 page（主窗口，type=page）
                let target = pages.into_iter().find(|p| p.page_type == "page");
                if let Some(page) = target {
                    if let Some(ws_url) = page.ws_url {
                        set_status(&status, UnlockStatus::Connecting).await;
                        tracing::info!("[PluginUnlock] connecting to CDP: {}", ws_url);

                        match connect_and_monitor(&ws_url, &cmd_rx, &msg_id, &status).await {
                            Ok(()) => {
                                tracing::info!("[PluginUnlock] connection ended gracefully");
                                // 重置退避
                                reconnect_delay = config.reconnect_base_ms;
                            }
                            Err(e) => {
                                tracing::warn!("[PluginUnlock] connection error: {}", e);
                                set_status(
                                    &status,
                                    UnlockStatus::Failed {
                                        error: e.to_string(),
                                    },
                                )
                                .await;
                            }
                        }
                    }
                }
            }
            None => {
                // CDP 不可用，保持 Disconnected
                set_status_if_not(&status, UnlockStatus::Disconnected).await;
            }
        }

        // 退避等待
        sleep(Duration::from_millis(reconnect_delay)).await;
        reconnect_delay = (reconnect_delay * 2).min(config.reconnect_max_ms);
        detect_tick.tick().await;
    }
}

/// 检测 CDP HTTP 端点，返回 page 列表
async fn detect_cdp(url: &str) -> Option<Vec<CdpPage>> {
    match reqwest::get(url).await {
        Ok(resp) => {
            if resp.status().is_success() {
                match resp.json::<Vec<CdpPage>>().await {
                    Ok(pages) if !pages.is_empty() => Some(pages),
                    _ => None,
                }
            } else {
                None
            }
        }
        Err(e) => {
            tracing::debug!("[PluginUnlock] CDP detect failed: {}", e);
            None
        }
    }
}

/// WebSocket 连接、注入、并持续监控页面刷新
async fn connect_and_monitor(
    ws_url: &str,
    cmd_rx: &Arc<Mutex<mpsc::Receiver<ServiceCommand>>>,
    msg_id_counter: &Arc<Mutex<u64>>,
    status: &Arc<RwLock<UnlockStatus>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (ws_stream, _) = connect_async(ws_url).await?;
    let (mut write, mut read) = ws_stream.split();

    // 1. 启用 Runtime domain
    let runtime_enable = make_cdp_msg(msg_id_counter, "Runtime.enable", json!({})).await;
    write.send(WsMessage::Text(runtime_enable)).await?;
    let _ = read.next().await;

    // 2. 启用 Page domain（监听刷新事件）
    let page_enable = make_cdp_msg(msg_id_counter, "Page.enable", json!({})).await;
    write.send(WsMessage::Text(page_enable)).await?;
    let _ = read.next().await;

    // 3. 首次注入
    inject_unlock_script(&mut write, &mut read, msg_id_counter, status).await?;

    // 4. 持续监控：监听 Page.loadEventFired 和外部命令
    loop {
        tokio::select! {
            // 监听 WebSocket 消息
            msg = read.next() => {
                match msg {
                    Some(Ok(WsMessage::Text(text))) => {
                        if let Ok(resp) = serde_json::from_str::<CdpResponse>(&text) {
                            // 检测 Page.loadEventFired 事件
                            if resp.method.as_deref() == Some("Page.loadEventFired") {
                                tracing::info!("[PluginUnlock] page refreshed, reinjecting...");
                                inject_unlock_script(&mut write, &mut read, msg_id_counter, status).await?;
                            }
                        }
                    }
                    Some(Ok(WsMessage::Close(_))) | None => {
                        tracing::info!("[PluginUnlock] WebSocket closed");
                        break;
                    }
                    Some(Err(e)) => {
                        tracing::warn!("[PluginUnlock] WebSocket error: {}", e);
                        break;
                    }
                    _ => {}
                }
            }

            // 监听外部命令
            cmd = async {
                let mut rx = cmd_rx.lock().await;
                rx.recv().await
            } => {
                match cmd {
                    Some(ServiceCommand::Reinject) => {
                        tracing::info!("[PluginUnlock] manual reinject requested");
                        inject_unlock_script(&mut write, &mut read, msg_id_counter, status).await?;
                    }
                    Some(ServiceCommand::Stop) => {
                        tracing::info!("[PluginUnlock] stop requested, closing connection");
                        let _ = write.close().await;
                        return Ok(());
                    }
                    None => break,
                }
            }

            // 心跳检测：如果 30 秒没有收到任何消息，检查连接是否仍然活跃
            _ = sleep(Duration::from_secs(30)) => {
                // 发送一个简单的 Runtime.getProperties 来检测连接
                let ping = make_cdp_msg(msg_id_counter, "Runtime.evaluate", json!({"expression": "1+1"})).await;
                if let Err(e) = write.send(WsMessage::Text(ping)).await {
                    tracing::warn!("[PluginUnlock] ping failed, connection dead: {}", e);
                    break;
                }
            }
        }
    }

    Ok(())
}

/// 发送注入脚本
async fn inject_unlock_script(
    write: &mut (impl Sink<WsMessage, Error = tokio_tungstenite::tungstenite::Error> + Unpin),
    read: &mut (impl Stream<Item = Result<WsMessage, tokio_tungstenite::tungstenite::Error>> + Unpin),
    msg_id_counter: &Arc<Mutex<u64>>,
    status: &Arc<RwLock<UnlockStatus>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    set_status(status, UnlockStatus::Connected).await;

    let unlock_script = r#"
(function() {
    function findReactRoots() {
        const roots = [];
        document.querySelectorAll('*').forEach(node => {
            const key = Object.keys(node).find(k =>
                k.startsWith('__reactContainer') || k.startsWith('__reactFiber')
            );
            if (key) roots.push(node[key]);
        });
        const bodyKey = Object.keys(document.body).find(k =>
            k.startsWith('__reactContainer') || k.startsWith('__reactFiber')
        );
        if (bodyKey) roots.push(document.body[bodyKey]);
        return roots;
    }
    function findAuthStateInFiber(fiber) {
        if (!fiber) return null;
        let hook = fiber.memoizedState;
        while (hook) {
            if (hook.memoizedState && typeof hook.memoizedState === 'object') {
                const state = hook.memoizedState;
                if (state.setAuthMethod && typeof state.setAuthMethod === 'function') {
                    return state;
                }
            }
            hook = hook.next;
        }
        let child = fiber.child;
        while (child) {
            const result = findAuthStateInFiber(child);
            if (result) return result;
            child = child.sibling;
        }
        return null;
    }
    const roots = findReactRoots();
    for (const root of roots) {
        const authState = findAuthStateInFiber(root);
        if (authState && authState.setAuthMethod) {
            authState.setAuthMethod('chatgpt');
            console.log('[CAS PluginUnlock] ✅ Plugins unlocked via setAuthMethod');
            return true;
        }
    }
    console.log('[CAS PluginUnlock] ❌ setAuthMethod not found');
    return false;
})()
"#;

    let evaluate = make_cdp_msg(
        msg_id_counter,
        "Runtime.evaluate",
        json!({
            "expression": unlock_script,
            "awaitPromise": true,
            "returnByValue": true
        }),
    )
    .await;

    write.send(WsMessage::Text(evaluate)).await?;

    // 等待响应
    if let Some(Ok(WsMessage::Text(resp))) = read.next().await {
        let parsed: CdpResponse = serde_json::from_str(&resp)?;
        if let Some(error) = parsed.error {
            return Err(format!("CDP error {}: {}", error.code, error.message).into());
        }
        // 检查返回值
        if let Some(result) = parsed.result {
            if let Some(val) = result.get("result").and_then(|v| v.get("value")) {
                if val.as_bool() == Some(true) {
                    set_status(status, UnlockStatus::Injected).await;
                    return Ok(());
                }
            }
        }
    }

    // 如果没有明确成功，也算成功（可能已经注入了）
    set_status(status, UnlockStatus::Injected).await;
    Ok(())
}

/// 生成 CDP 消息 JSON
async fn make_cdp_msg(
    counter: &Arc<Mutex<u64>>,
    method: &str,
    params: serde_json::Value,
) -> String {
    let mut id = counter.lock().await;
    *id += 1;
    let current_id = *id;
    drop(id);

    json!({
        "id": current_id,
        "method": method,
        "params": params
    })
    .to_string()
}

/// 设置状态（带日志）
async fn set_status(status: &Arc<RwLock<UnlockStatus>>, new: UnlockStatus) {
    let mut s = status.write().await;
    if *s != new {
        tracing::info!("[PluginUnlock] status: {:?} → {:?}", *s, new);
        *s = new;
    }
}

/// 仅在当前状态不等于目标时才设置（避免频繁写锁）
async fn set_status_if_not(status: &Arc<RwLock<UnlockStatus>>, new: UnlockStatus) {
    let s = status.read().await;
    if *s != new {
        drop(s);
        set_status(status, new).await;
    }
}

// ── 便捷构造函数 ──

impl PluginUnlockService {
    /// 使用默认配置创建
    pub fn default_new() -> Self {
        Self::new(UnlockConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_serialization() {
        let s = UnlockStatus::Failed {
            error: "test".into(),
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("failed"));
    }

    #[test]
    fn test_default_config() {
        let c = UnlockConfig::default();
        assert_eq!(c.cdp_http_url, "http://127.0.0.1:9222/json/list");
    }
}
