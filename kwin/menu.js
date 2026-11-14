// Tessera's entries in the window menu (Alt+F3 or a right click on the title
// bar). KWin only offers this to plain JavaScript scripts, not to declarative
// ones like main.qml, so it is a second, small script. KWin shows script
// entries under "Extensions".
//
// Installed and updated by the service; edit `kwin/menu.js` in the repository.

function centered(w) {
    const area = workspace.clientArea(KWin.MaximizeArea, w);
    const g = w.frameGeometry;
    const width = Math.min(g.width, area.width);
    const height = Math.min(g.height, area.height);
    return {
        x: Math.round(area.x + (area.width - width) / 2),
        y: Math.round(area.y + (area.height - height) / 2),
        width: width,
        height: height,
    };
}

function center(w) {
    if (!w || w.fullScreen) return;
    if (w.tile) w.tile.unmanage(w);
    if (w.maximizeMode !== 0) w.setMaximize(false, false);
    w.frameGeometry = centered(w);
}

registerUserActionsMenu(function (w) {
    if (!w || !w.normalWindow || !w.moveable || w.fullScreen) return null;
    return {
        text: "Center Window",
        triggered: function () {
            center(w);
        },
    };
});
