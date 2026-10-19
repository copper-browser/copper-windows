//! UI Automation: a fragment provider for every accessible node, rooted at
//! each window, with the page's own provider grafted under its slot.
//!
//! UI Automation calls providers on the UI thread (they are not
//! free-threaded), so a provider reaches the window's state through its
//! host's `peek`: a read or a small change, never the app's handler.
//! When the state is busy, or the node has gone, a call fails with
//! `UIA_E_ELEMENTNOTAVAILABLE`, which clients expect.

// The SDK's constant names (`UIA_NamePropertyId`) are matched as written.
#![allow(non_upper_case_globals)]

use std::collections::HashMap;
use std::rc::Weak;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::{ClientToScreen, ScreenToClient};
use windows::Win32::System::Com::SAFEARRAY;
use windows::Win32::System::Ole::{SafeArrayCreateVector, SafeArrayPutElement};
use windows::Win32::System::Variant::{VARIANT, VT_I4};
use windows::Win32::UI::Accessibility::*;
use windows::core::{BOOL, BSTR, Error, HRESULT, IUnknown, Interface, PCWSTR, Result, implement};
use windows_core::IUnknownImpl;

use super::ui::{Command, Ui};
use super::window::Host;
use crate::a11y::View;
use crate::chrome::CaptionButton;
use crate::event::Event;
use crate::geom::Point;
use crate::node::Role;
use crate::tree::{NodeId, Tree};

/// Which tree a provider stands in: the window's or the open menu's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Space {
    Main = 1,
    Menu = 2,
}

fn gone() -> Error {
    Error::from_hresult(HRESULT(UIA_E_ELEMENTNOTAVAILABLE as i32))
}

/// A null interface out-parameter with success.
fn none<T>() -> Result<T> {
    Err(Error::empty())
}

fn listening() -> bool {
    // SAFETY: a plain query.
    unsafe { UiaClientsAreListening().as_bool() }
}

fn raise(p: &IRawElementProviderSimple, event: UIA_EVENT_ID) {
    // SAFETY: raising an event on our own provider.
    unsafe {
        let _ = UiaRaiseAutomationEvent(p, event);
    }
}

fn disconnect(p: &IRawElementProviderSimple) {
    // SAFETY: as above.
    unsafe {
        let _ = UiaDisconnectProvider(p);
    }
}

/// The providers a window has handed out, so the same node is always the
/// same element and removed ones can be disconnected.
pub(super) struct Bridge {
    host: Weak<Host>,
    hwnd: HWND,
    root: Option<IRawElementProviderSimple>,
    menu_root: Option<(HWND, IRawElementProviderSimple)>,
    elements: HashMap<(Space, NodeId), IRawElementProviderSimple>,
    view: View,
    menu_view: View,
}

impl Bridge {
    pub fn new(host: Weak<Host>, hwnd: HWND) -> Bridge {
        Bridge {
            host,
            hwnd,
            root: None,
            menu_root: None,
            elements: HashMap::new(),
            view: View::new(),
            menu_view: View::new(),
        }
    }

    fn element(&mut self, space: Space, id: NodeId) -> IRawElementProviderSimple {
        let host = self.host.clone();
        self.elements
            .entry((space, id))
            .or_insert_with(|| Element { host, space, id }.into())
            .clone()
    }

    fn root(&mut self, space: Space) -> Option<IRawElementProviderSimple> {
        match space {
            Space::Main => {
                let host = self.host.clone();
                let hwnd = self.hwnd;
                Some(
                    self.root
                        .get_or_insert_with(|| Root { host, space, hwnd }.into())
                        .clone(),
                )
            }
            Space::Menu => self.menu_root.as_ref().map(|(_, r)| r.clone()),
        }
    }

    /// Focus moved to `id`.
    pub fn focus(&mut self, tree: &Tree, id: NodeId) {
        if listening() && accessible(tree, id) {
            let e = self.element(Space::Main, id);
            raise(&e, UIA_AutomationFocusChangedEventId);
        }
    }

    /// `id` was invoked (click, Enter, Space).
    pub fn invoked(&mut self, tree: &Tree, id: NodeId) {
        if listening() && accessible(tree, id) {
            let e = self.element(Space::Main, id);
            raise(&e, UIA_Invoke_InvokedEventId);
        }
    }

    /// A menu opened in popup `hwnd`; `card` is its `Role::Menu` node.
    pub fn menu_opened(&mut self, hwnd: HWND, card: NodeId) {
        self.menu_closed();
        let root = Root {
            host: self.host.clone(),
            space: Space::Menu,
            hwnd,
        }
        .into();
        self.menu_root = Some((hwnd, root));
        if listening() {
            let e = self.element(Space::Menu, card);
            raise(&e, UIA_MenuOpenedEventId);
        }
    }

    /// Row `row` of the menu is highlighted: menus report it as focus.
    pub fn menu_highlight(&mut self, row: NodeId) {
        if listening() && self.menu_root.is_some() {
            let e = self.element(Space::Menu, row);
            raise(&e, UIA_AutomationFocusChangedEventId);
        }
    }

    /// The menu closed: let go of its providers.
    pub fn menu_closed(&mut self) {
        let Some((hwnd, root)) = self.menu_root.take() else {
            return;
        };
        self.elements.retain(|k, p| {
            let keep = k.0 != Space::Menu;
            if !keep {
                disconnect(p);
            }
            keep
        });
        disconnect(&root);
        // SAFETY: telling UI Automation the popup's root is gone.
        unsafe {
            UiaReturnRawElementProvider(
                hwnd,
                WPARAM(0),
                LPARAM(0),
                None::<&IRawElementProviderSimple>,
            );
        }
    }

    /// Let go of providers whose nodes left the tree.
    pub fn prune(&mut self, tree: &Tree) {
        if self.elements.is_empty() {
            return;
        }
        self.elements.retain(|k, p| {
            let keep = k.0 != Space::Main || tree.contains(k.1);
            if !keep {
                disconnect(p);
            }
            keep
        });
    }

    /// The window is going away.
    pub fn teardown(&mut self) {
        self.menu_closed();
        for p in self.elements.values() {
            disconnect(p);
        }
        self.elements.clear();
        if let Some(r) = self.root.take() {
            disconnect(&r);
        }
        // SAFETY: telling UI Automation the window's root is gone.
        unsafe {
            UiaReturnRawElementProvider(
                self.hwnd,
                WPARAM(0),
                LPARAM(0),
                None::<&IRawElementProviderSimple>,
            );
        }
    }
}

fn accessible(tree: &Tree, id: NodeId) -> bool {
    tree.get(id).is_some_and(|n| n.a11y.role != Role::None)
}

fn tree_of(ui: &Ui, space: Space) -> Option<&Tree> {
    match space {
        Space::Main => Some(&ui.tree),
        Space::Menu => ui.menu.as_ref().map(|m| m.tree()),
    }
}

fn hwnd_of(ui: &Ui, space: Space) -> Option<HWND> {
    match space {
        Space::Main => Some(ui.hwnd),
        Space::Menu => ui.menu.as_ref().map(|m| m.hwnd()),
    }
}

/// The accessibility view of `space`, brought up to date.
fn view_of(ui: &mut Ui, space: Space) -> Option<(&View, &Tree)> {
    let Ui {
        uia, tree, menu, ..
    } = ui;
    match space {
        Space::Main => {
            uia.view.sync(tree);
            Some((&uia.view, tree))
        }
        Space::Menu => {
            let t = menu.as_ref()?.tree();
            uia.menu_view.sync(t);
            Some((&uia.menu_view, t))
        }
    }
}

/// Where a navigation lands.
enum Hop {
    Nowhere,
    Root,
    Node(NodeId),
    /// The page's own provider (under a slot).
    Page(IUnknown),
}

fn hop_to(ui: &mut Ui, space: Space, hop: Hop) -> Result<IRawElementProviderFragment> {
    match hop {
        Hop::Nowhere => none(),
        Hop::Root => ui.uia.root(space).ok_or_else(gone)?.cast(),
        Hop::Node(n) => ui.uia.element(space, n).cast(),
        Hop::Page(p) => p.cast(),
    }
}

/// The page provider of slot `id`, if it is one and has a page.
fn page_of(ui: &Ui, id: NodeId) -> Option<IUnknown> {
    ui.slots.get(&id).and_then(|(h, _)| h.automation())
}

fn control_type(role: Role) -> UIA_CONTROLTYPE_ID {
    match role {
        Role::None | Role::Group => UIA_GroupControlTypeId,
        Role::Button => UIA_ButtonControlTypeId,
        Role::Text => UIA_TextControlTypeId,
        Role::Edit => UIA_EditControlTypeId,
        Role::List => UIA_ListControlTypeId,
        Role::ListItem => UIA_ListItemControlTypeId,
        Role::Tab => UIA_TabControlTypeId,
        Role::TabItem => UIA_TabItemControlTypeId,
        Role::Menu => UIA_MenuControlTypeId,
        Role::MenuItem => UIA_MenuItemControlTypeId,
        Role::ToolTip => UIA_ToolTipControlTypeId,
        Role::Pane => UIA_PaneControlTypeId,
        Role::ScrollBar => UIA_ScrollBarControlTypeId,
        Role::TitleBar => UIA_TitleBarControlTypeId,
        Role::Document => UIA_DocumentControlTypeId,
        Role::Image => UIA_ImageControlTypeId,
    }
}

/// Whether a node can be invoked.
fn invokable(tree: &Tree, id: NodeId) -> bool {
    tree.get(id).is_some_and(|n| {
        matches!(
            n.a11y.role,
            Role::Button | Role::MenuItem | Role::TabItem | Role::ListItem
        ) && n.field_ref().is_none()
            && !n.is_slot()
    })
}

/// A runtime id: unique within the window, stable for the node's life.
fn runtime_id(space: Space, id: NodeId) -> Result<*mut SAFEARRAY> {
    let raw = id.raw();
    let parts = [
        UiaAppendRuntimeId as i32,
        space as i32,
        raw as u32 as i32,
        (raw >> 32) as u32 as i32,
    ];
    // SAFETY: a fresh vector of four I4s, filled in place.
    unsafe {
        let sa = SafeArrayCreateVector(VT_I4, 0, parts.len() as u32);
        if sa.is_null() {
            return Err(Error::from_hresult(
                windows::Win32::Foundation::E_OUTOFMEMORY,
            ));
        }
        for (i, v) in parts.iter().enumerate() {
            let at = i as i32;
            SafeArrayPutElement(sa, &at, (v as *const i32).cast())?;
        }
        Ok(sa)
    }
}

/// A node's provider.
#[implement(
    IRawElementProviderSimple,
    IRawElementProviderFragment,
    IInvokeProvider,
    IValueProvider
)]
struct Element {
    host: Weak<Host>,
    space: Space,
    id: NodeId,
}

impl Element {
    fn with<R>(&self, f: impl FnOnce(&mut Ui) -> Result<R>) -> Result<R> {
        let host = self.host.upgrade().ok_or_else(gone)?;
        let r = host.peek(|ui| {
            if tree_of(ui, self.space).is_some_and(|t| t.contains(self.id)) {
                f(ui)
            } else {
                Err(gone())
            }
        });
        r.unwrap_or_else(|| Err(gone()))
    }
}

impl IRawElementProviderSimple_Impl for Element_Impl {
    fn ProviderOptions(&self) -> Result<ProviderOptions> {
        Ok(ProviderOptions_ServerSideProvider)
    }

    fn GetPatternProvider(&self, pattern: UIA_PATTERN_ID) -> Result<IUnknown> {
        let (invoke, value) = self.with(|ui| {
            let t = tree_of(ui, self.space).ok_or_else(gone)?;
            let field = t.get(self.id).is_some_and(|n| n.field_ref().is_some());
            Ok((invokable(t, self.id), field))
        })?;
        if pattern == UIA_InvokePatternId && invoke {
            return Ok(self.to_interface::<IInvokeProvider>().into());
        }
        if pattern == UIA_ValuePatternId && value {
            return Ok(self.to_interface::<IValueProvider>().into());
        }
        none()
    }

    fn GetPropertyValue(&self, property: UIA_PROPERTY_ID) -> Result<VARIANT> {
        self.with(|ui| {
            let window_focus = ui.focused_window;
            let t = tree_of(ui, self.space).ok_or_else(gone)?;
            let n = t.get(self.id).ok_or_else(gone)?;
            let v = match property {
                UIA_ControlTypePropertyId => VARIANT::from(control_type(n.a11y.role).0),
                UIA_NamePropertyId => VARIANT::from(n.a11y.name.as_str()),
                UIA_HelpTextPropertyId if !n.a11y.help.is_empty() => {
                    VARIANT::from(n.a11y.help.as_str())
                }
                UIA_AutomationIdPropertyId if n.tag != 0 => {
                    VARIANT::from(format!("copper-{}", n.tag).as_str())
                }
                UIA_IsKeyboardFocusablePropertyId => {
                    VARIANT::from(n.flags.focusable && !n.flags.disabled)
                }
                UIA_HasKeyboardFocusPropertyId => {
                    let focused = match self.space {
                        Space::Main => window_focus && t.focused() == Some(self.id),
                        Space::Menu => n.flags.selected,
                    };
                    VARIANT::from(focused)
                }
                UIA_IsEnabledPropertyId => VARIANT::from(!n.flags.disabled),
                UIA_IsOffscreenPropertyId => {
                    VARIANT::from(!t.is_shown(self.id) || t.visible_rect(self.id).is_empty())
                }
                UIA_IsPasswordPropertyId => VARIANT::from(false),
                UIA_FrameworkIdPropertyId => VARIANT::from("Copper"),
                UIA_IsInvokePatternAvailablePropertyId => VARIANT::from(invokable(t, self.id)),
                UIA_ValueValuePropertyId => match n.field_ref() {
                    Some(f) => VARIANT::from(f.edit.text()),
                    None => VARIANT::default(),
                },
                _ => VARIANT::default(),
            };
            Ok(v)
        })
    }

    fn HostRawElementProvider(&self) -> Result<IRawElementProviderSimple> {
        none()
    }
}

impl IRawElementProviderFragment_Impl for Element_Impl {
    fn Navigate(&self, direction: NavigateDirection) -> Result<IRawElementProviderFragment> {
        let space = self.space;
        let id = self.id;
        // Work out where to go with the state borrowed; ask the page's
        // provider (another process's proxy) only after it is released.
        let hop = self.with(|ui| {
            let page = if matches!(
                direction,
                NavigateDirection_FirstChild | NavigateDirection_LastChild
            ) {
                page_of(ui, id)
            } else {
                None
            };
            let (view, _) = view_of(ui, space).ok_or_else(gone)?;
            let hop = match direction {
                NavigateDirection_Parent => view.parent(id).map_or(Hop::Root, Hop::Node),
                NavigateDirection_NextSibling => {
                    view.sibling(id, 1).map_or(Hop::Nowhere, Hop::Node)
                }
                NavigateDirection_PreviousSibling => {
                    view.sibling(id, -1).map_or(Hop::Nowhere, Hop::Node)
                }
                NavigateDirection_FirstChild => match page {
                    Some(p) => Hop::Page(p),
                    None => view.first_child(Some(id)).map_or(Hop::Nowhere, Hop::Node),
                },
                NavigateDirection_LastChild => match page {
                    Some(p) => Hop::Page(p),
                    None => view.last_child(Some(id)).map_or(Hop::Nowhere, Hop::Node),
                },
                _ => Hop::Nowhere,
            };
            Ok(match hop {
                Hop::Page(p) => Err(p),
                h => Ok(hop_to(ui, space, h)),
            })
        })?;
        match hop {
            Ok(r) => r,
            Err(page) => page.cast(),
        }
    }

    fn GetRuntimeId(&self) -> Result<*mut SAFEARRAY> {
        runtime_id(self.space, self.id)
    }

    fn BoundingRectangle(&self) -> Result<UiaRect> {
        self.with(|ui| {
            let s = ui.scale();
            let hwnd = hwnd_of(ui, self.space).ok_or_else(gone)?;
            let t = tree_of(ui, self.space).ok_or_else(gone)?;
            if !t.is_shown(self.id) {
                return Ok(UiaRect::default());
            }
            let r = t.visible_rect(self.id);
            let mut o = POINT::default();
            // SAFETY: converting a point for our own window.
            unsafe {
                let _ = ClientToScreen(hwnd, &mut o);
            }
            Ok(UiaRect {
                left: f64::from(o.x) + f64::from(r.x * s),
                top: f64::from(o.y) + f64::from(r.y * s),
                width: f64::from(r.w * s),
                height: f64::from(r.h * s),
            })
        })
    }

    fn GetEmbeddedFragmentRoots(&self) -> Result<*mut SAFEARRAY> {
        Ok(std::ptr::null_mut())
    }

    fn SetFocus(&self) -> Result<()> {
        self.with(|ui| {
            if self.space == Space::Main {
                ui.set_focus(Some(self.id), true);
                ui.commands.push(Command::FocusWindow);
            }
            Ok(())
        })
    }

    fn FragmentRoot(&self) -> Result<IRawElementProviderFragmentRoot> {
        self.with(|ui| ui.uia.root(self.space).ok_or_else(gone)?.cast())
    }
}

impl IInvokeProvider_Impl for Element_Impl {
    fn Invoke(&self) -> Result<()> {
        self.with(|ui| {
            match self.space {
                Space::Main => {
                    match ui.caption.which(self.id) {
                        Some(CaptionButton::Minimize) => ui.commands.push(Command::Minimize),
                        Some(CaptionButton::Maximize) => ui.commands.push(Command::ToggleMaximize),
                        Some(CaptionButton::Close) => ui.events.push_back(Event::CloseRequested),
                        None => ui.events.push_back(Event::Click(self.id)),
                    }
                    let Ui { uia, tree, .. } = ui;
                    uia.invoked(tree, self.id);
                }
                Space::Menu => {
                    let item = ui.menu.as_ref().and_then(|m| m.item_of_row(self.id));
                    if let Some(item) = item {
                        ui.choose(item);
                    }
                }
            }
            Ok(())
        })
    }
}

impl IValueProvider_Impl for Element_Impl {
    fn SetValue(&self, val: &PCWSTR) -> Result<()> {
        // SAFETY: UI Automation passes a NUL-terminated string.
        let s = unsafe { val.to_string() }
            .map_err(|_| Error::from_hresult(windows::Win32::Foundation::E_INVALIDARG))?;
        self.with(|ui| {
            if self.space == Space::Main {
                ui.set_field_text(self.id, &s, false);
                ui.events.push_back(Event::FieldChanged(self.id));
            }
            Ok(())
        })
    }

    fn Value(&self) -> Result<BSTR> {
        self.with(|ui| {
            let t = tree_of(ui, self.space).ok_or_else(gone)?;
            let text = t
                .get(self.id)
                .and_then(|n| n.field_ref())
                .map_or("", |f| f.edit.text());
            Ok(BSTR::from(text))
        })
    }

    fn IsReadOnly(&self) -> Result<BOOL> {
        Ok(BOOL::from(false))
    }
}

/// A window's root: the window's own (host) provider supplies its name,
/// bounds and control type; this one supplies the children.
#[implement(
    IRawElementProviderSimple,
    IRawElementProviderFragment,
    IRawElementProviderFragmentRoot
)]
struct Root {
    host: Weak<Host>,
    space: Space,
    hwnd: HWND,
}

impl Root {
    fn with<R>(&self, f: impl FnOnce(&mut Ui) -> Result<R>) -> Result<R> {
        let host = self.host.upgrade().ok_or_else(gone)?;
        host.peek(f).unwrap_or_else(|| Err(gone()))
    }
}

impl IRawElementProviderSimple_Impl for Root_Impl {
    fn ProviderOptions(&self) -> Result<ProviderOptions> {
        Ok(ProviderOptions_ServerSideProvider)
    }

    fn GetPatternProvider(&self, _pattern: UIA_PATTERN_ID) -> Result<IUnknown> {
        none()
    }

    fn GetPropertyValue(&self, property: UIA_PROPERTY_ID) -> Result<VARIANT> {
        Ok(match property {
            UIA_FrameworkIdPropertyId => VARIANT::from("Copper"),
            UIA_ControlTypePropertyId if self.space == Space::Menu => {
                VARIANT::from(UIA_MenuControlTypeId.0)
            }
            _ => VARIANT::default(),
        })
    }

    fn HostRawElementProvider(&self) -> Result<IRawElementProviderSimple> {
        // SAFETY: the host provider of our own window.
        unsafe { UiaHostProviderFromHwnd(self.hwnd) }
    }
}

impl IRawElementProviderFragment_Impl for Root_Impl {
    fn Navigate(&self, direction: NavigateDirection) -> Result<IRawElementProviderFragment> {
        self.with(|ui| {
            let (view, _) = view_of(ui, self.space).ok_or_else(gone)?;
            let hop = match direction {
                NavigateDirection_FirstChild => {
                    view.first_child(None).map_or(Hop::Nowhere, Hop::Node)
                }
                NavigateDirection_LastChild => {
                    view.last_child(None).map_or(Hop::Nowhere, Hop::Node)
                }
                _ => Hop::Nowhere,
            };
            hop_to(ui, self.space, hop)
        })
    }

    fn GetRuntimeId(&self) -> Result<*mut SAFEARRAY> {
        Ok(std::ptr::null_mut())
    }

    fn BoundingRectangle(&self) -> Result<UiaRect> {
        Ok(UiaRect::default())
    }

    fn GetEmbeddedFragmentRoots(&self) -> Result<*mut SAFEARRAY> {
        Ok(std::ptr::null_mut())
    }

    fn SetFocus(&self) -> Result<()> {
        Ok(())
    }

    fn FragmentRoot(&self) -> Result<IRawElementProviderFragmentRoot> {
        Ok(self.to_interface())
    }
}

/// What a hit test found.
enum Found {
    Root,
    Node(NodeId),
    Page(IUnknown),
}

impl IRawElementProviderFragmentRoot_Impl for Root_Impl {
    fn ElementProviderFromPoint(&self, x: f64, y: f64) -> Result<IRawElementProviderFragment> {
        let space = self.space;
        let hwnd = self.hwnd;
        let found = self.with(|ui| {
            let s = ui.scale();
            let mut p = POINT {
                x: x.round() as i32,
                y: y.round() as i32,
            };
            // SAFETY: converting a point for our own window.
            unsafe {
                let _ = ScreenToClient(hwnd, &mut p);
            }
            let at = Point::new(p.x as f32 / s, p.y as f32 / s);
            let (view, tree) = view_of(ui, space).ok_or_else(gone)?;
            // The last node in document order that contains the point is
            // the one painted on top.
            let hit = view
                .all()
                .iter()
                .rev()
                .copied()
                .find(|&id| tree.is_shown(id) && tree.visible_rect(id).contains(at));
            Ok(match hit {
                Some(id) => match page_of(ui, id) {
                    Some(page) => Found::Page(page),
                    None => Found::Node(id),
                },
                None => Found::Root,
            })
        })?;
        match found {
            Found::Root => Ok(self.to_interface()),
            Found::Node(id) => self.with(|ui| ui.uia.element(space, id).cast()),
            Found::Page(page) => {
                // Let the page find its own element under the point.
                if let Ok(root) = page.cast::<IRawElementProviderFragmentRoot>() {
                    // SAFETY: a call on the page's provider, no state borrowed.
                    if let Ok(e) = unsafe { root.ElementProviderFromPoint(x, y) } {
                        return Ok(e);
                    }
                }
                page.cast()
            }
        }
    }

    fn GetFocus(&self) -> Result<IRawElementProviderFragment> {
        self.with(|ui| {
            let focused = match self.space {
                Space::Main => ui.tree.focused(),
                Space::Menu => ui.menu.as_ref().and_then(|m| m.highlighted_row()),
            };
            let (view, _) = view_of(ui, self.space).ok_or_else(gone)?;
            match focused.filter(|&f| view.contains(f)) {
                Some(f) => ui.uia.element(self.space, f).cast(),
                None => none(),
            }
        })
    }
}

impl Ui {
    /// `WM_GETOBJECT` on the main window.
    pub(super) fn get_object(&mut self, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
        if lp.0 as u32 as i32 != UiaRootObjectId {
            return None;
        }
        let root = self.uia.root(Space::Main)?;
        // SAFETY: handing UI Automation our root for this request.
        Some(unsafe { UiaReturnRawElementProvider(self.hwnd, wp, lp, &root) })
    }

    /// `WM_GETOBJECT` on the menu popup `hwnd`.
    pub(super) fn popup_object(&mut self, hwnd: HWND, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
        if lp.0 as u32 as i32 != UiaRootObjectId {
            return None;
        }
        let (h, root) = self.uia.menu_root.as_ref()?;
        if *h != hwnd {
            return None;
        }
        let root = root.clone();
        // SAFETY: as above.
        Some(unsafe { UiaReturnRawElementProvider(hwnd, wp, lp, &root) })
    }
}
