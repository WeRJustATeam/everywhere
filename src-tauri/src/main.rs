use tauri::Manager;
use airplay_protocol::Server;

fn main() {
    let server = Server::new();

    server.on("error", |err| {
        eprintln!("Error: {:?}", err);
    });

    server.on("listening", || {
        println!("AirPlay server is listening on port 7000");
    });

    server.on("deviceOn", |device| {
        println!("Device found: {:?}", device.info);
        device.play("http://your-video-url", 0, |err| {
            if let Some(err) = err {
                eprintln!("Error playing: {:?}", err);
            }
        });
    });

    server.start();

    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
} 