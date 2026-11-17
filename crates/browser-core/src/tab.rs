//! Tab collection and lifecycle rules.

use serde::{Deserialize, Serialize};

use crate::navigation::NavigationState;

/// Stable tab identity used by the UI to associate engine instances.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TabId(u64);

impl TabId {
    pub fn get(self) -> u64 {
        self.0
    }
}

/// A tab's persistent model. The live engine is owned by the app layer and keyed
/// by `TabId`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tab {
    pub id: TabId,
    pub navigation: NavigationState,
    pub pinned: bool,
}

impl Tab {
    fn new(id: TabId, url: impl Into<String>) -> Self {
        Self {
            id,
            navigation: NavigationState::new(url),
            pinned: false,
        }
    }
}

/// A tab that can be reopened from the "recently closed" list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClosedTab {
    pub url: String,
    pub title: String,
    /// Position it occupied, used to restore it near its old neighbours.
    pub index: usize,
}

/// The browser's open tabs and active selection.
#[derive(Debug, Default, Clone)]
pub struct TabSet {
    tabs: Vec<Tab>,
    active: Option<TabId>,
    next_id: u64,
    recently_closed: Vec<ClosedTab>,
    max_closed: usize,
}

impl TabSet {
    pub fn new() -> Self {
        Self {
            max_closed: 20,
            ..Self::default()
        }
    }

    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }

    pub fn active_id(&self) -> Option<TabId> {
        self.active
    }

    pub fn active_tab(&self) -> Option<&Tab> {
        let id = self.active?;
        self.tab(id)
    }

    pub fn active_tab_mut(&mut self) -> Option<&mut Tab> {
        let id = self.active?;
        self.tabs.iter_mut().find(|tab| tab.id == id)
    }

    pub fn tab(&self, id: TabId) -> Option<&Tab> {
        self.tabs.iter().find(|tab| tab.id == id)
    }

    pub fn tab_mut(&mut self, id: TabId) -> Option<&mut Tab> {
        self.tabs.iter_mut().find(|tab| tab.id == id)
    }

    pub fn index_of(&self, id: TabId) -> Option<usize> {
        self.tabs.iter().position(|tab| tab.id == id)
    }

    /// Opens a tab at the end and activates it.
    pub fn open(&mut self, url: impl Into<String>) -> TabId {
        let id = self.allocate_id();
        self.tabs.push(Tab::new(id, url));
        self.active = Some(id);
        id
    }

    /// Inserts a tab at a specific position and activates it.
    pub fn open_at(&mut self, index: usize, url: impl Into<String>) -> TabId {
        let id = self.allocate_id();
        let index = index.min(self.tabs.len());
        self.tabs.insert(index, Tab::new(id, url));
        self.active = Some(id);
        id
    }

    fn allocate_id(&mut self) -> TabId {
        let id = TabId(self.next_id);
        self.next_id += 1;
        id
    }

    /// Closes a tab and selects a neighbour when the active tab was closed.
    ///
    /// Returns the closed tab for the "recently closed" stack.
    pub fn close(&mut self, id: TabId) -> Option<ClosedTab> {
        let index = self.index_of(id)?;
        let tab = self.tabs.remove(index);
        let closed = ClosedTab {
            url: tab.navigation.url.clone(),
            title: tab.navigation.display_title(),
            index,
        };
        self.recently_closed.push(closed.clone());
        if self.recently_closed.len() > self.max_closed {
            self.recently_closed.remove(0);
        }

        if self.active == Some(id) {
            self.active = self
                .tabs
                .get(index)
                .or_else(|| {
                    index
                        .checked_sub(1)
                        .and_then(|previous| self.tabs.get(previous))
                })
                .map(|tab| tab.id);
        }
        Some(closed)
    }

    pub fn close_active(&mut self) -> Option<ClosedTab> {
        let id = self.active?;
        self.close(id)
    }

    /// Activates a tab. Returns false when the id is unknown.
    pub fn activate(&mut self, id: TabId) -> bool {
        if self.tab(id).is_some() {
            self.active = Some(id);
            true
        } else {
            false
        }
    }

    pub fn activate_index(&mut self, index: usize) -> bool {
        match self.tabs.get(index) {
            Some(tab) => {
                self.active = Some(tab.id);
                true
            }
            None => false,
        }
    }

    /// Cycles to the next tab, wrapping around. Returns the new active id.
    pub fn activate_next(&mut self) -> Option<TabId> {
        if self.tabs.is_empty() {
            return None;
        }
        let current = self.active_index().unwrap_or(0);
        let next = (current + 1) % self.tabs.len();
        self.active = Some(self.tabs[next].id);
        self.active
    }

    pub fn activate_previous(&mut self) -> Option<TabId> {
        if self.tabs.is_empty() {
            return None;
        }
        let current = self.active_index().unwrap_or(0);
        let previous = (current + self.tabs.len() - 1) % self.tabs.len();
        self.active = Some(self.tabs[previous].id);
        self.active
    }

    pub fn active_index(&self) -> Option<usize> {
        self.active.and_then(|id| self.index_of(id))
    }

    /// Moves a tab to a new index, keeping it active.
    pub fn move_tab(&mut self, from: usize, to: usize) -> bool {
        if from >= self.tabs.len() || to >= self.tabs.len() || from == to {
            return false;
        }
        let tab = self.tabs.remove(from);
        self.tabs.insert(to, tab);
        true
    }

    /// Duplicates a tab next to the original and activates the copy.
    pub fn duplicate(&mut self, id: TabId) -> Option<TabId> {
        let index = self.index_of(id)?;
        let url = self.tabs[index].navigation.url.clone();
        let pinned = self.tabs[index].pinned;
        let new_id = self.allocate_id();
        let mut tab = Tab::new(new_id, url);
        tab.pinned = pinned;
        self.tabs.insert(index + 1, tab);
        self.active = Some(new_id);
        Some(new_id)
    }

    pub fn recently_closed(&self) -> &[ClosedTab] {
        &self.recently_closed
    }

    /// Reopens the most recently closed tab at its original position.
    pub fn reopen_closed(&mut self) -> Option<TabId> {
        let closed = self.recently_closed.pop()?;
        let index = closed.index.min(self.tabs.len());
        let id = self.allocate_id();
        let mut tab = Tab::new(id, closed.url);
        tab.navigation.title = closed.title;
        self.tabs.insert(index, tab);
        self.active = Some(id);
        Some(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_tabs_activates_the_new_tab() {
        let mut set = TabSet::new();
        let first = set.open("https://example.com");
        let second = set.open("https://rust-lang.org");
        assert_eq!(set.len(), 2);
        assert_eq!(set.active_id(), Some(second));
        assert_ne!(first, second);
        assert_eq!(
            set.active_tab().unwrap().navigation.url,
            "https://rust-lang.org"
        );
    }

    #[test]
    fn closing_active_tab_selects_the_next_tab() {
        let mut set = TabSet::new();
        let first = set.open("https://a.test");
        let second = set.open("https://b.test");
        let third = set.open("https://c.test");
        set.activate(second);
        set.close(second);
        assert_eq!(set.active_id(), Some(third));
        assert_eq!(set.len(), 2);
        assert!(set.tab(first).is_some());
    }

    #[test]
    fn closing_last_tab_selects_the_previous_tab() {
        let mut set = TabSet::new();
        let first = set.open("https://a.test");
        let second = set.open("https://b.test");
        set.close(second);
        assert_eq!(set.active_id(), Some(first));
    }

    #[test]
    fn closing_the_only_tab_leaves_the_set_empty() {
        let mut set = TabSet::new();
        let id = set.open("https://a.test");
        assert!(set.close(id).is_some());
        assert!(set.is_empty());
        assert_eq!(set.active_id(), None);
    }

    #[test]
    fn closing_an_inactive_tab_keeps_the_active_one() {
        let mut set = TabSet::new();
        let first = set.open("https://a.test");
        let second = set.open("https://b.test");
        set.close(first);
        assert_eq!(set.active_id(), Some(second));
    }

    #[test]
    fn tab_cycling_wraps() {
        let mut set = TabSet::new();
        let first = set.open("https://a.test");
        let second = set.open("https://b.test");
        assert_eq!(set.activate_next(), Some(first));
        assert_eq!(set.activate_next(), Some(second));
        assert_eq!(set.activate_previous(), Some(first));
    }

    #[test]
    fn move_tab_reorders() {
        let mut set = TabSet::new();
        let a = set.open("https://a.test");
        let b = set.open("https://b.test");
        let c = set.open("https://c.test");
        assert!(set.move_tab(0, 2));
        let ids: Vec<TabId> = set.tabs().iter().map(|tab| tab.id).collect();
        assert_eq!(ids, vec![b, c, a]);
        assert!(!set.move_tab(0, 9));
    }

    #[test]
    fn duplicate_places_copy_after_original() {
        let mut set = TabSet::new();
        let a = set.open("https://a.test");
        set.tab_mut(a).unwrap().navigation.title = "A".into();
        let copy = set.duplicate(a).expect("duplicate");
        assert_eq!(set.len(), 2);
        assert_eq!(set.active_id(), Some(copy));
        assert_eq!(set.tabs()[1].id, copy);
        assert_eq!(set.tabs()[1].navigation.url, "https://a.test");
    }

    #[test]
    fn recently_closed_tabs_restore_at_their_index() {
        let mut set = TabSet::new();
        set.open("https://a.test");
        set.open("https://b.test");
        let c = set.open("https://c.test");
        set.close(c);
        let restored = set.reopen_closed().expect("reopen");
        assert_eq!(set.active_id(), Some(restored));
        assert_eq!(set.index_of(restored), Some(2));
        assert_eq!(set.tabs()[2].navigation.url, "https://c.test");
    }

    #[test]
    fn recently_closed_is_bounded() {
        let mut set = TabSet::new();
        for index in 0..30 {
            let id = set.open(format!("https://tab{index}.test"));
            set.close(id);
        }
        assert_eq!(set.recently_closed().len(), 20);
        assert_eq!(
            set.recently_closed().last().unwrap().url,
            "https://tab29.test"
        );
    }
}
