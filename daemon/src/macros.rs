#[macro_export]
macro_rules! handle_method {
    ($name:ident, $event:path) => {
        pub async fn $name(&self) -> $crate::error::Result<()> {
            let event = $event;
            self.tx.send(event).await?;
            Ok(())
        }
    };
    ($name:ident, $event:ident, $($arg_name:ident: $arg_type:ty),* $(,)?) => {
        pub async fn $name(&self, $($arg_name: $arg_type),*) -> $crate::error::Result<()> {
            let event = $event {
                $($arg_name: $arg_name),*
            };
            self.tx.send(event).await?;
            Ok(())
        }
    };
}
