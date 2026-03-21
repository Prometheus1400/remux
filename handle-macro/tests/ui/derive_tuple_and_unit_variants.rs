use handle_macro::Handle;

type Result<T> = std::result::Result<T, tokio::sync::mpsc::error::SendError<TestEvent>>;

#[derive(Handle, Debug)]
enum TestEvent {
    Tick,
    Data(u8, u16),
}

fn main() {
    let (_tx, _rx) = tokio::sync::mpsc::channel::<TestEvent>(1);
}
