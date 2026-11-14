# Changelog

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## Unreleased

### Added
- "Center Window" in the window menu (right click on the title bar or
  Alt+F3, under Extensions). Takes the window out of its zone or maximised
  state and centres it on its screen.

## 0.1.2 — 2026-11-09

### Changed
- Size and position are only remembered when they were set by hand. Programs
  otherwise keep their own size, and KWin places them as usual.
- A second window of a program with a different title (a friends list, a
  launcher's splash) no longer takes the main window's place.

### Fixed
- Windows in a zone had the zone's size saved as their own, so they later
  opened almost full screen.
- Windows that resize themselves after opening were forced back to the saved
  size several times.
- Places saved by earlier versions without this distinction no longer change
  size or position.

## 0.1.1 — 2026-11-09

### Fixed
- Closing a window did not reach the service, so the next window of that
  program found its remembered place still taken and opened where KWin put it.
- An updated KWin script could stay unstarted after other scripts had been
  turned off in the same session.

## 0.1.0 — 2026-11-09

### Added
- Zone editor: templates (columns, rows, grid, priority, main + stack),
  splitting by click, dividers to drag, spacing, saved layouts, undo.
  Layouts are applied as KWin tiles per screen and virtual desktop.
- Windows open where they were last closed: position, size, screen, virtual
  desktop, zone, maximised and full-screen state. Several windows per program.
- Per program: remember, always open in a fixed zone, or leave alone.
- Arrangements: save every window's place under a name and restore it, from
  the window, the tray menu, a shortcut or the command line.
- A maximised window returns to its zone when it is restored.
- Shortcuts for moving the active window between zones.
- Tray icon, start at login, updates from the settings.
