//! What the Windows setup program (src/bin/retro-wall-setup) knows that is not Windows: the settings it
//! shows and the files they are kept in (`settings`), the program and themes it carries in itself
//! (`payload`), and the scheduled task that starts the wall (`task`). Here so that it is tested on every
//! system; the program around it, its window and what it does to Windows, is Windows only.

pub mod payload;
pub mod settings;
pub mod task;
