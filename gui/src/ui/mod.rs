//! Components taken from rust-ui (<https://rust-ui.com/docs/components>, the Dioxus registry,
//! MIT License, Copyright (c) 2026 Max Wells). The Tailwind classes are rust-ui's; hone's
//! design tokens stand behind them in `assets/input.css`. Changes against rust-ui: variants the
//! GUI does not use are left out, the icon crate is replaced by inline SVG, router links and
//! pointer handlers are left out, `Input` and `Textarea` report their value through `oninput`,
//! and the malformed second selector of `TableCell` is fixed. `Combobox` is hone's own: rust-ui's
//! Select needs an inline script.

mod alert;
mod badge;
mod button;
mod card;
mod checkbox;
mod combobox;
mod input;
mod progress;
mod separator;
mod skeleton;
mod table;
mod tabs;
mod textarea;

pub use alert::{Alert, AlertDescription, AlertTitle};
pub use badge::{Badge, BadgeVariant};
pub use button::{Button, ButtonSize, ButtonVariant};
pub use card::{Card, CardContent, CardHeader, CardTitle};
pub use checkbox::Checkbox;
pub use combobox::{Choice, Combobox};
pub use input::Input;
pub use progress::Progress;
pub use separator::Separator;
pub use skeleton::Skeleton;
pub use table::{Table, TableBody, TableCell, TableHead, TableHeader, TableRow, TableWrapper};
pub use tabs::{Tabs, TabsContent, TabsList, TabsTrigger, TabsVariant};
pub use textarea::Textarea;
