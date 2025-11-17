use std::sync::Arc;

use tokio::sync::Mutex;

type NodeSharedPtr<T> = Option<Arc<Mutex<Box<Node<T>>>>>;

struct Node<T> {
    val: T,
    left: NodeSharedPtr<T>,
    right: NodeSharedPtr<T>,
}

pub struct Tree<T> {
    root: NodeSharedPtr<T>,
}
