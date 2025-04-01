use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use futures::{StreamExt, SinkExt};
use tokio::sync::mpsc;
use warp::ws::{Message, WebSocket};
use warp::Filter;

// 客户端类型
struct Client {
    room_id: String,
    sender: mpsc::UnboundedSender<Message>,
}

type Clients = Arc<Mutex<HashMap<String, Client>>>;

#[tokio::main]
async fn main() {
    // 存储所有连接的客户端
    let clients: Clients = Arc::new(Mutex::new(HashMap::new()));
    
    // WebSocket 路由 - 支持 /ws/房间ID 格式
    let ws_route = warp::path("ws")
        .and(warp::path::param::<String>())  // 捕获房间ID作为路径参数
        .and(warp::ws())
        .and(with_clients(clients.clone()))
        .map(|room_id: String, ws: warp::ws::Ws, clients| {
            ws.on_upgrade(move |socket| handle_connection(socket, room_id, clients))
        });
    
    // 启动服务器
    println!("WebSocket 服务器运行在 ws://127.0.0.1:3030/ws/房间ID");
    warp::serve(ws_route)
        .run(([127, 0, 0, 1], 3030))
        .await;
}

// 辅助函数，将 clients 传递给处理函数
fn with_clients(clients: Clients) -> impl Filter<Extract = (Clients,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || clients.clone())
}

// 处理新的 WebSocket 连接
async fn handle_connection(ws: WebSocket, room_id: String, clients: Clients) {
    println!("新连接进入房间: {}", room_id);
    
    // 生成客户端 ID
    let client_id = uuid::Uuid::new_v4().to_string();
    println!("新客户端已连接: {}, 房间ID: {}", client_id, room_id);
    
    // 将WebSocket处理逻辑分为发送和接收两部分
    let (mut ws_sender, mut ws_receiver) = ws.split();
    
    // 创建一个通道，用于向WebSocket发送消息
    let (tx, mut rx) = mpsc::unbounded_channel();
    
    // 创建客户端并存储到客户端列表中
    {
        let mut clients_lock = clients.lock().unwrap();
        clients_lock.insert(
            client_id.clone(),
            Client { 
                room_id: room_id.clone(),
                sender: tx.clone() 
            }
        );
    }
    
    // 启动一个任务来处理消息发送
    let client_id_clone = client_id.clone();
    let sender_task = tokio::spawn(async move {
        while let Some(message) = rx.recv().await {
            match ws_sender.send(message).await {
                Ok(_) => {}, // 消息发送成功
                Err(e) => {
                    eprintln!("向客户端 {} 发送消息失败: {}", client_id_clone, e);
                    break;
                }
            }
        }
    });
    
    // 处理接收到的消息
    while let Some(result) = ws_receiver.next().await {
        match result {
            Ok(msg) => {
                // 处理不同类型的消息
                if let Ok(text) = msg.to_str() {
                    println!("收到来自房间 {} 客户端 {} 的消息: {}", room_id, client_id, text);
                    broadcast_message(&clients, &room_id, &client_id, text).await;
                }
            }
            Err(e) => {
                eprintln!("WebSocket 错误: {}", e);
                break;
            }
        }
    }
    
    // 客户端断开连接，清理资源
    let mut clients_lock = clients.lock().unwrap();
    clients_lock.remove(&client_id);
    
    // 终止发送任务
    sender_task.abort();
    println!("客户端已断开连接: {}, 房间ID: {}", client_id, room_id);
}

// 广播消息给同一房间内的其他客户端
async fn broadcast_message(clients: &Clients, room_id: &str, sender_id: &str, message: &str) {
    let locked_clients = clients.lock().unwrap();
    
    for (client_id, client) in locked_clients.iter() {
        // 只发送给同一房间的其他客户端
        if client_id != sender_id && client.room_id == room_id {
            println!("发送消息到客户端: {}, 房间: {}", client_id, room_id);
            if let Err(e) = client.sender.send(Message::text(message)) {
                eprintln!("向客户端 {} 发送消息失败: {}", client_id, e);
            }
        }
    }
}
