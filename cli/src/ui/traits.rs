use ratatui::widgets::StatefulWidget;
use terminput::Event;

pub enum Selection {
    Index(usize),
    Cancelled,
}

pub trait SelectorStatefulWidget: StatefulWidget {
    fn input(event: Event, state: &mut Self::State) -> Option<Selection>;
}
