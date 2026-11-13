//! Apps and the providers that know them (docs/EDGE_APPS.md section 3.1): which cards an app gives,
//! and the registry every card file adds one line to.

use crate::app::App;
use crate::card::{Card, Source};

/// How a provider says which apps are its own.
#[derive(Clone, Copy, Debug)]
pub enum Claim {
    /// the desktop entry id, exactly
    Id(&'static str),
    /// ids that start with this (Telegram's entry id carries a hash after its name)
    IdPrefix(&'static str),
    /// every one of these is in the entry's Categories
    Categories(&'static [&'static str]),
}

/// Whether the provider itself is offered in "Add a card", beside the apps it claims.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Listed {
    /// Infynitae's own part with no desktop entry: Settings, Sound, Calendar
    Always,
    /// only while no installed app is claimed by it
    WithoutApp,
    /// only through its apps, or never by name
    Never,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Size {
    S,
    M,
    L,
}

impl Size {
    pub fn letter(self) -> &'static str {
        match self {
            Size::S => "s",
            Size::M => "m",
            Size::L => "l",
        }
    }
    pub fn of_letter(s: &str) -> Option<Size> {
        match s.to_ascii_lowercase().as_str() {
            "s" => Some(Size::S),
            "m" => Some(Size::M),
            "l" => Some(Size::L),
            _ => None,
        }
    }
}

/// One kind of card a provider gives.
#[derive(Clone, Copy, Debug)]
pub struct CardKind {
    /// the word after the '/'
    pub id: &'static str,
    /// the header, and the name in "Add a card"
    pub title: &'static str,
    /// one line for "Add a card": what it shows and does
    pub about: &'static str,
    /// smallest first, at least one
    pub sizes: &'static [Size],
    pub default: Size,
    /// may sit at half the panel's width
    pub half: bool,
    /// the data it needs, by the provider's source keys
    pub sources: &'static [&'static str],
}

pub trait Provider: Sync {
    /// The word before the '/' when a card is not tied to one app: "settings", "music".
    fn id(&self) -> &'static str;
    /// Its name where it is listed without an app: "Settings", "System monitor".
    fn name(&self) -> &'static str;
    fn claims(&self) -> &'static [Claim];
    fn listed(&self) -> Listed;
    fn cards(&self) -> &'static [CardKind];
    /// A new card of `kind`; None for a kind or an argument it does not know.
    fn card(&self, kind: &str, argument: Option<&str>) -> Option<Box<dyn Card>>;
    /// The source behind `key`; None for a key it does not know.
    fn source(&self, key: &str) -> Option<Box<dyn Source>>;

    fn kind(&self, id: &str) -> Option<&'static CardKind> {
        self.cards().iter().find(|k| k.id == id)
    }
}

/// Every provider, in the order claims are tried. A card file adds its provider here.
pub fn providers() -> &'static [&'static dyn Provider] {
    &[
        &crate::cards::settings::Settings,
        &crate::cards::sound::Sound,
    ]
}

pub fn by_id(id: &str) -> Option<&'static dyn Provider> {
    providers().iter().copied().find(|p| p.id() == id)
}

/// The provider that owns `app`: claims by id and id prefix over every provider first, then by
/// categories (an app claimed by name is never taken by a category). None: the simple card's.
pub fn owner_of(app: &App) -> Option<&'static dyn Provider> {
    let named = providers().iter().copied().find(|p| {
        p.claims().iter().any(|c| match c {
            Claim::Id(id) => app.id == *id,
            Claim::IdPrefix(pre) => app.id.starts_with(pre),
            Claim::Categories(_) => false,
        })
    });
    named.or_else(|| {
        providers().iter().copied().find(|p| {
            p.claims().iter().any(|c| match c {
                Claim::Categories(cats) => cats
                    .iter()
                    .all(|want| app.categories.iter().any(|have| have == want)),
                _ => false,
            })
        })
    })
}

/// A placed card's provider and the app it is tied to: the owner word is a provider id (the card
/// uses no one app), or an installed app's id (its provider). None: an app not installed, or a
/// provider that does not exist.
pub fn resolve<'a>(
    owner: &str,
    apps: &'a [App],
) -> Option<(&'static dyn Provider, Option<&'a App>)> {
    if let Some(p) = by_id(owner) {
        return Some((p, None));
    }
    let app = apps.iter().find(|a| a.id == owner)?;
    owner_of(app).map(|p| (p, Some(app)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(id: &str, cats: &[&str]) -> App {
        App {
            id: id.into(),
            name: id.into(),
            categories: cats.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn sizes_read_and_print_as_letters() {
        assert_eq!(Size::of_letter("M"), Some(Size::M));
        assert_eq!(Size::of_letter("x"), None);
        assert_eq!(Size::L.letter(), "l");
    }

    #[test]
    fn a_provider_id_resolves_to_its_provider_and_no_app() {
        let (p, a) = resolve("settings", &[]).unwrap();
        assert_eq!(p.id(), "settings");
        assert!(a.is_none());
        assert!(resolve("nothing-installed", &[app("steam", &["Game"])]).is_none());
    }
}
