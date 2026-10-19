//! Virtual-key codes to [`Key`]s, and the modifier state.

use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, VIRTUAL_KEY, VK_APPS, VK_BACK, VK_CONTROL, VK_DELETE, VK_DOWN, VK_END, VK_ESCAPE,
    VK_F1, VK_F24, VK_HOME, VK_INSERT, VK_LEFT, VK_MENU, VK_NEXT, VK_PRIOR, VK_RETURN, VK_RIGHT,
    VK_SHIFT, VK_SPACE, VK_TAB, VK_UP,
};

use crate::input::{Key, Mods};

/// The key for virtual-key code `vk`.
pub fn key(vk: u32) -> Key {
    let v = VIRTUAL_KEY(vk as u16);
    match v {
        VK_LEFT => Key::Left,
        VK_RIGHT => Key::Right,
        VK_UP => Key::Up,
        VK_DOWN => Key::Down,
        VK_HOME => Key::Home,
        VK_END => Key::End,
        VK_PRIOR => Key::PageUp,
        VK_NEXT => Key::PageDown,
        VK_BACK => Key::Backspace,
        VK_DELETE => Key::Delete,
        VK_INSERT => Key::Insert,
        VK_RETURN => Key::Enter,
        VK_ESCAPE => Key::Escape,
        VK_TAB => Key::Tab,
        VK_SPACE => Key::Space,
        VK_APPS => Key::Menu,
        _ if (0x41..=0x5A).contains(&vk) => Key::Letter(char::from(vk as u8)),
        _ if (0x30..=0x39).contains(&vk) => Key::Digit((vk - 0x30) as u8),
        _ if (VK_F1.0 as u32..=VK_F24.0 as u32).contains(&vk) => {
            Key::F((vk - VK_F1.0 as u32 + 1) as u8)
        }
        _ => Key::Other(vk),
    }
}

fn down(vk: VIRTUAL_KEY) -> bool {
    // SAFETY: reads this thread's keyboard state.
    unsafe { GetKeyState(i32::from(vk.0)) < 0 }
}

/// The modifiers held right now (as of the message being handled).
pub fn mods() -> Mods {
    Mods {
        ctrl: down(VK_CONTROL),
        shift: down(VK_SHIFT),
        alt: down(VK_MENU),
    }
}
