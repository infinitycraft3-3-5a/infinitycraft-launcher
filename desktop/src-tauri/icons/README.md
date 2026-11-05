# Icons

Tauri requires PNG icons (128x128, 256x256) plus a Windows .ico before
`npx tauri build` can run the bundler. Generate them once with:

```
npx tauri icon "C:\Users\larit\Documents\WOW\LOGO-InfinityCraft.jpg"
```

`tauri icon` reads the source image and populates:

- 32x32.png, 128x128.png, 128x128@2x.png, icon.png (256x256)
- icon.ico (multi-resolution Windows icon)
- Square30x30.png, Square150x150.png, ... (Store assets)

Do NOT commit the generated PNGs unless you intend to ship them via git.
