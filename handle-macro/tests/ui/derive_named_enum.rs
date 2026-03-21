use handle_macro::Handle;

type Result<T> = std::result::Result<T, tokio::sync::mpsc::error::SendError<TestEvent>>;

#[derive(Handle, Debug)]
enum TestEvent {
    Rename { id: u32, name: String },
}

fn main() {
    let (_tx, _rx) = tokio::sync::mpsc::channel::<TestEvent>(1);
}
