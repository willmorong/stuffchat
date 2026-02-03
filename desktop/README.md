# Stuffchat Desktop

Desktop client for Stuffchat, built with Electron.

## Development

Install dependencies:
```bash
npm install
```

Run the app:
```bash
npm start
```

## Building

Build for your current platform:
```bash
npm run build
```

Build for specific platforms:
```bash
npm run build:linux   # Linux (AppImage, deb)
npm run build:mac     # macOS (dmg, zip)
npm run build:win     # Windows (nsis, portable)
```

Built packages will be in the `dist/` directory.

## Features

- Wraps the Stuffchat web app at https://chat.stuffcity.org
- External links open in your default browser
- Cross-platform support (Linux, macOS, Windows)
