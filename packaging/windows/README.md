# Windows resources

`torro-workplace.ico`, `torro-calendar.ico` and `torro-tasks.ico` are embedded into the executables by the
`build.rs` of each crate (`crates/*/build.rs`, using `winresource`), together with product name, version and
publisher. Explorer, the taskbar and SmartScreen show them.

Each `.ico` holds the sizes 16, 24, 32, 48, 64, 128 and 256 as PNG images with transparency (valid for Windows Vista
and later). They are made from the placeholder artwork in `../linux/icons/*.svg`. When the real artwork replaces the
SVGs, make the `.ico` files again, for example with ImageMagick:

```sh
magick -background none ../linux/icons/torro-tasks.svg \
  -define icon:auto-resize=256,128,64,48,32,24,16 torro-tasks.ico
```

`build.rs` only does something when the build target is Windows; it needs a resource compiler there (`rc.exe` from the
Windows SDK with MSVC, or `llvm-rc` when cross building).
