// Tessera's half inside KWin.
//
// On Wayland only the compositor may move windows, so this script does the
// moving and the Tessera service does the remembering. They talk over D-Bus:
// the script calls the service for every event, and fetches commands with a
// long-poll on `Next`, which the service answers when it has something to do
// or after 20 seconds with an empty string.
//
// Installed and updated by the service; edit `kwin/main.qml` in the repository.
import QtQuick
import QtQml
import org.kde.kwin

Item {
    id: root

    readonly property string version: "@VERSION@"
    property bool connected: false
    property var cfg: ({ zone_after_unmaximize: true })
    property var trackers: ({})

    // ------------------------------------------------------------ D-Bus --

    Component {
        id: callComponent
        DBusCall {
            id: dbusCall
            property var done: null
            property var fail: null
            service: "app.tessera.Daemon"
            path: "/app/tessera/Daemon"
            dbusInterface: "app.tessera.Daemon1"
            onFinished: (ret) => {
                const f = dbusCall.done;
                dbusCall.destroy();
                if (f) f(ret && ret.length ? ret[0] : "");
            }
            onFailed: {
                const f = dbusCall.fail;
                dbusCall.destroy();
                if (f) f();
            }
        }
    }

    function call(method, args, done, fail) {
        const c = callComponent.createObject(root, { method: method, arguments: args });
        c.done = done || null;
        c.fail = fail || null;
        c.call();
    }

    function hello() {
        call("Hello", [version], function (ret) {
            connected = true;
            try { cfg = JSON.parse(ret); } catch (e) {}
            sendTopology();
            const list = [];
            const all = Workspace.windows;
            for (let i = 0; i < all.length; i++) {
                if (managed(all[i])) list.push(info(all[i]));
            }
            call("Existing", [JSON.stringify(list)]);
            poll();
        }, function () {
            connected = false;
            retry.restart();
        });
    }

    function poll() {
        call("Next", [], function (ret) {
            if (ret) {
                try { handle(JSON.parse(ret)); } catch (e) { console.log("tessera: " + e); }
            }
            poll();
        }, function () {
            connected = false;
            retry.restart();
        });
    }

    Timer {
        id: retry
        interval: 5000
        onTriggered: root.hello()
    }

    function handle(m) {
        switch (m.cmd) {
        case "config":
            cfg = m.config;
            break;
        case "zones":
            reportZones();
            break;
        case "apply_zones":
            applyZones(m);
            reportZones();
            break;
        case "snapshot": {
            const list = [];
            const all = Workspace.windows;
            for (let i = 0; i < all.length; i++) {
                if (managed(all[i])) list.push(info(all[i]));
            }
            call("Report", ["snapshot:" + m.req, JSON.stringify(list)]);
            break;
        }
        case "place":
            for (let i = 0; i < m.items.length; i++) {
                const w = windowById(m.items[i].id);
                if (w) startRestore(w, m.items[i].restore);
            }
            break;
        case "activate":
            activate(m["class"]);
            break;
        }
    }

    // ---------------------------------------------------------- windows --

    // Windows Tessera remembers. `deleted` is already set while KWin reports a
    // window as removed, so closing checks `remembered` instead of `managed`.
    function remembered(w) {
        return w && w.normalWindow && !w.transient && !w.skipTaskbar
            && w.resourceClass && w.resourceClass.length > 0;
    }

    function managed(w) {
        return remembered(w) && !w.deleted;
    }

    function wid(w) {
        return String(w.internalId);
    }

    function windowById(id) {
        const all = Workspace.windows;
        for (let i = 0; i < all.length; i++) {
            if (wid(all[i]) === id) return all[i];
        }
        return null;
    }

    function desktopIds(w) {
        const out = [];
        const ds = w.desktops;
        for (let i = 0; i < ds.length; i++) out.push(ds[i].id);
        return out;
    }

    function deskOf(w) {
        const ds = w.desktops;
        return ds && ds.length === 1 ? ds[0] : Workspace.currentDesktop;
    }

    function tileRef(w) {
        const t = w.tile;
        if (!t) return null;
        const r = t.relativeGeometry;
        return {
            output: w.output ? w.output.name : "",
            desktop: deskOf(w).id,
            rect: [r.x, r.y, r.width, r.height]
        };
    }

    function info(w) {
        const tr = trackers[wid(w)];
        // A zone or maximising decides the size then, not the window: report
        // the size it had on its own.
        const plain = w.maximizeMode === 0 && !w.fullScreen && !w.tile;
        const g = (!plain && tr && tr.normal) ? tr.normal : w.frameGeometry;
        return {
            id: wid(w),
            "class": w.resourceClass,
            desktop_file: w.desktopFileName || "",
            caption: w.caption || "",
            pid: w.pid,
            output: w.output ? w.output.name : "",
            x: Math.round(g.x), y: Math.round(g.y),
            w: Math.round(g.width), h: Math.round(g.height),
            desktops: desktopIds(w),
            all_desktops: w.onAllDesktops,
            max: w.maximizeMode,
            full: w.fullScreen,
            min: w.minimized,
            above: w.keepAbove,
            tile: tileRef(w),
            // Only what the user changed by hand counts as remembered size and
            // position; programs keep sizing themselves otherwise.
            user_pos: tr ? tr.userPos : false,
            user_size: tr ? tr.userSize : false
        };
    }

    function screenByName(name) {
        const ss = Workspace.screens;
        for (let i = 0; i < ss.length; i++) {
            if (ss[i].name === name) return ss[i];
        }
        return null;
    }

    function desktopById(id) {
        const ds = Workspace.desktops;
        for (let i = 0; i < ds.length; i++) {
            if (ds[i].id === id) return ds[i];
        }
        return null;
    }

    function activate(cls) {
        const all = Workspace.windows;
        for (let i = 0; i < all.length; i++) {
            const w = all[i];
            if (w.resourceClass === cls && w.normalWindow) {
                if (w.minimized) w.minimized = false;
                Workspace.activeWindow = w;
            }
        }
    }

    // ---------------------------------------------------------- restore --

    function clampRect(r, area) {
        const width = Math.min(r.width, area.width);
        const height = Math.min(r.height, area.height);
        const x = Math.max(area.x, Math.min(r.x, area.x + area.width - width));
        const y = Math.max(area.y, Math.min(r.y, area.y + area.height - height));
        return { x: x, y: y, width: width, height: height };
    }

    function findTile(ref, w) {
        const screen = screenByName(ref.output) || w.output;
        const desk = desktopById(ref.desktop) || deskOf(w);
        const root = Workspace.rootTile(screen, desk);
        if (!root) return null;
        const ls = leaves(root, []);
        for (let i = 0; i < ls.length; i++) {
            const r = ls[i].relativeGeometry;
            if (Math.abs(r.x - ref.rect[0]) < 0.02 && Math.abs(r.y - ref.rect[1]) < 0.02
                && Math.abs(r.width - ref.rect[2]) < 0.02 && Math.abs(r.height - ref.rect[3]) < 0.02)
                return ls[i];
        }
        return null;
    }

    // Puts the window where `r` says. Returns the geometry to hold it at, or
    // null when it went into a zone or needs no further help.
    function applyRestore(w, r) {
        if (!w || w.deleted || !r) return null;
        const p = r.placement || {};

        if (r.pin) {
            const screen = screenByName(r.pin.output) || w.output;
            const ls = leaves(Workspace.rootTile(screen, deskOf(w)), []);
            if (r.pin.index >= 1 && r.pin.index <= ls.length) ls[r.pin.index - 1].manage(w);
            return null;
        }

        if (r.desktop) {
            if (p.all_desktops) {
                w.onAllDesktops = true;
            } else if (p.desktops && p.desktops.length) {
                const ds = [];
                for (let i = 0; i < p.desktops.length; i++) {
                    const d = desktopById(p.desktops[i]);
                    if (d) ds.push(d);
                }
                if (ds.length) w.desktops = ds;
            }
        }

        const remembered = screenByName(p.output);
        const dest = (r.screen && remembered) ? remembered : w.output;

        if (r.zone && p.tile) {
            const t = findTile(p.tile, w);
            if (t) {
                t.manage(w);
                if (r.above) w.keepAbove = p.above;
                return null;
            }
        }

        let target = null;
        const usePos = r.geometry && (r.force_geometry || p.user_pos);
        const useSize = r.geometry && (r.force_geometry || p.user_size);
        if ((usePos || useSize) && p.w > 0 && p.h > 0) {
            const cur = w.frameGeometry;
            const width = useSize ? p.w : cur.width;
            const height = useSize ? p.h : cur.height;
            // A new size alone stays centred where KWin put the window.
            let x = usePos ? p.x : cur.x + (cur.width - width) / 2;
            let y = usePos ? p.y : cur.y + (cur.height - height) / 2;
            // Same spot relative to the screen when it lands on another one.
            if (usePos && remembered && dest !== remembered) {
                x += dest.geometry.x - remembered.geometry.x;
                y += dest.geometry.y - remembered.geometry.y;
            }
            const rect = clampRect({ x: x, y: y, width: width, height: height },
                                   Workspace.clientArea(KWin.MaximizeArea, usePos ? dest : w.output, deskOf(w)));
            if (w.tile) w.tile.unmanage(w);
            w.frameGeometry = Qt.rect(rect.x, rect.y, rect.width, rect.height);
            target = { rect: rect, pos: usePos, size: useSize };
        } else if (r.screen && (r.force_geometry || p.user_pos) && remembered && w.output !== remembered) {
            Workspace.sendClientToScreen(w, remembered);
        }

        if (r.maximized) {
            if (p.full && w.fullScreenable) {
                w.fullScreen = true;
                target = null;
            } else if (p.max && w.maximizable) {
                w.setMaximize((p.max & 1) !== 0, (p.max & 2) !== 0);
                target = null;
            }
        }
        if (r.above) w.keepAbove = p.above;
        if (r.minimized && p.min) w.minimized = true;
        return target;
    }

    function startRestore(w, r) {
        const target = applyRestore(w, r);
        const tr = trackers[wid(w)];
        if (tr && target) tr.hold(target);
    }

    // ------------------------------------------------------------ zones --

    function leaves(t, out) {
        if (!t) return out;
        if (t.tiles.length === 0) {
            out.push(t);
        } else {
            for (let i = 0; i < t.tiles.length; i++) leaves(t.tiles[i], out);
        }
        return out;
    }

    function toTree(t) {
        if (t.tiles.length === 0) return { s: 1, d: 1 };
        const dir = t.layoutDirection === 2 ? 2 : 1;
        const g = t.relativeGeometry;
        const c = [];
        for (let i = 0; i < t.tiles.length; i++) {
            const child = toTree(t.tiles[i]);
            const r = t.tiles[i].relativeGeometry;
            child.s = dir === 2 ? r.height / Math.max(g.height, 1e-9) : r.width / Math.max(g.width, 1e-9);
            c.push(child);
        }
        return { s: 1, d: dir, c: c };
    }

    function reportZones() {
        const out = [];
        const ss = Workspace.screens;
        const ds = Workspace.desktops;
        for (let i = 0; i < ss.length; i++) {
            for (let j = 0; j < ds.length; j++) {
                const root = Workspace.rootTile(ss[i], ds[j]);
                if (!root) continue;
                out.push({ output: ss[i].name, desktop: ds[j].id, tree: toTree(root), padding: Math.round(root.padding) });
            }
        }
        call("Report", ["zones", JSON.stringify(out)]);
    }

    function clearTile(t) {
        let guard = 0;
        while (t.tiles.length > 0 && guard++ < 500) {
            const c = t.tiles[t.tiles.length - 1];
            clearTile(c);
            if (t.tiles.length > 0 && t.tiles[t.tiles.length - 1] === c) c.remove();
        }
    }

    function build(t, node) {
        const n = node.c ? node.c.length : 0;
        if (n < 2) return;
        t.split(node.d);
        let guard = 0;
        while (t.tiles.length < n && guard++ < 64) t.tiles[t.tiles.length - 1].split(node.d);
        const g = t.absoluteGeometry;
        const columns = node.d !== 2;
        const total = columns ? g.width : g.height;
        const start = columns ? g.x : g.y;
        let acc = 0;
        for (let i = 0; i < n - 1 && i < t.tiles.length - 1; i++) {
            acc += node.c[i].s;
            const c = t.tiles[i].absoluteGeometry;
            const end = columns ? c.x + c.width : c.y + c.height;
            const delta = start + acc * total - end;
            if (Math.abs(delta) >= 1) t.tiles[i].resizeByPixels(delta, columns ? Qt.RightEdge : Qt.BottomEdge);
        }
        for (let i = 0; i < n && i < t.tiles.length; i++) build(t.tiles[i], node.c[i]);
    }

    function setPadding(t, px) {
        t.padding = px;
        for (let i = 0; i < t.tiles.length; i++) setPadding(t.tiles[i], px);
    }

    function overlap(a, b) {
        const w = Math.min(a.x + a.width, b.x + b.width) - Math.max(a.x, b.x);
        const h = Math.min(a.y + a.height, b.y + b.height) - Math.max(a.y, b.y);
        return w > 0 && h > 0 ? w * h : 0;
    }

    function rebuild(root, tree, padding) {
        // Windows in the old zones move to whichever new zone covers most of
        // where they were, instead of dropping out of tiling.
        const saved = [];
        const old = leaves(root, []);
        for (let i = 0; i < old.length; i++) {
            const r = old[i].relativeGeometry;
            for (let j = 0; j < old[i].windows.length; j++) {
                saved.push({ w: old[i].windows[j], r: { x: r.x, y: r.y, width: r.width, height: r.height } });
            }
        }
        clearTile(root);
        build(root, tree);
        setPadding(root, padding);
        const fresh = leaves(root, []);
        for (let i = 0; i < saved.length; i++) {
            let best = null, bestArea = 0;
            for (let j = 0; j < fresh.length; j++) {
                const a = overlap(fresh[j].relativeGeometry, saved[i].r);
                if (a > bestArea) { bestArea = a; best = fresh[j]; }
            }
            if (best && saved[i].w && !saved[i].w.deleted) best.manage(saved[i].w);
        }
    }

    function applyZones(m) {
        const screen = screenByName(m.output);
        if (!screen) return;
        const ds = Workspace.desktops;
        for (let i = 0; i < ds.length; i++) {
            if (m.desktop && ds[i].id !== m.desktop) continue;
            const root = Workspace.rootTile(screen, ds[i]);
            if (root) rebuild(root, m.tree, m.padding || 0);
        }
    }

    function zoneTarget(step, absolute) {
        const w = Workspace.activeWindow;
        if (!w || !w.normalWindow) return;
        const ls = leaves(Workspace.rootTile(w.output, Workspace.currentDesktop), []);
        if (!ls.length) return;
        let index;
        if (absolute) {
            if (step > ls.length) return;
            index = step - 1;
        } else {
            const current = ls.indexOf(w.tile);
            if (current < 0) index = step > 0 ? 0 : ls.length - 1;
            else index = (current + step + ls.length) % ls.length;
        }
        if (w.maximizeMode !== 0) w.setMaximize(false, false);
        if (w.fullScreen) w.fullScreen = false;
        ls[index].manage(w);
    }

    // --------------------------------------------------------- tracking --

    Component {
        id: trackerComponent
        Item {
            id: tracker
            property var win
            property var normal: null
            property bool freeze: false
            property var savedTile: null
            property bool backToTile: false
            property var holdRect: null
            property int holdRound: 0
            property bool userMoving: false
            // Set once the user has moved or resized the window by hand.
            property bool userPos: false
            property bool userSize: false
            property var moveStart: null

            function snapshotNormal() {
                const g = win.frameGeometry;
                normal = { x: g.x, y: g.y, width: g.width, height: g.height };
            }

            function plainState() {
                return win.maximizeMode === 0 && !win.fullScreen && !win.tile;
            }

            function hold(target) {
                holdRect = target;
                holdRound = 0;
                holdTimer.interval = 150;
                holdTimer.restart();
            }

            function changed() {
                if (!freeze && plainState()) snapshotNormal();
                saveTimer.restart();
            }

            Timer {
                id: saveTimer
                interval: 700
                onTriggered: {
                    if (root.connected && root.managed(tracker.win))
                        root.call("Changed", [JSON.stringify(root.info(tracker.win))]);
                }
            }

            // Some programs move or size themselves once more after they
            // appear. What the user set is put back a few times, unless the
            // user has taken the window by then; the rest stays the program's.
            Timer {
                id: holdTimer
                onTriggered: {
                    const w = tracker.win;
                    const t = tracker.holdRect;
                    if (!t || !w || w.deleted || tracker.userMoving || w.tile
                        || w.maximizeMode !== 0 || w.fullScreen) return;
                    const g = w.frameGeometry;
                    const r = t.rect;
                    const want = {
                        x: t.pos ? r.x : g.x, y: t.pos ? r.y : g.y,
                        width: t.size ? r.width : g.width, height: t.size ? r.height : g.height
                    };
                    if (Math.abs(g.x - want.x) > 1 || Math.abs(g.y - want.y) > 1
                        || Math.abs(g.width - want.width) > 1 || Math.abs(g.height - want.height) > 1)
                        w.frameGeometry = Qt.rect(want.x, want.y, want.width, want.height);
                    tracker.holdRound++;
                    if (tracker.holdRound < 3) {
                        holdTimer.interval = tracker.holdRound === 1 ? 350 : 700;
                        holdTimer.restart();
                    }
                }
            }

            Connections {
                target: tracker.win
                function onFrameGeometryChanged() { tracker.changed(); }
                function onTileChanged() { tracker.changed(); }
                function onFullScreenChanged() { tracker.changed(); }
                function onDesktopsChanged() { tracker.changed(); }
                function onOutputChanged() { tracker.changed(); }
                function onMinimizedChanged() { tracker.changed(); }
                function onKeepAboveChanged() { tracker.changed(); }
                function onInteractiveMoveResizeStarted() {
                    tracker.userMoving = true;
                    const g = tracker.win.frameGeometry;
                    tracker.moveStart = tracker.plainState()
                        ? { x: g.x, y: g.y, width: g.width, height: g.height } : null;
                }
                function onInteractiveMoveResizeFinished() {
                    tracker.userMoving = false;
                    // Dropped into a zone or snapped to maximised: that is
                    // remembered as such, not as a size.
                    if (tracker.plainState()) {
                        const g = tracker.win.frameGeometry;
                        const s = tracker.moveStart;
                        if (!s) {
                            tracker.userPos = true;
                        } else {
                            if (Math.abs(g.x - s.x) > 2 || Math.abs(g.y - s.y) > 2) tracker.userPos = true;
                            if (Math.abs(g.width - s.width) > 2 || Math.abs(g.height - s.height) > 2) tracker.userSize = true;
                        }
                    }
                    tracker.moveStart = null;
                    tracker.changed();
                }
                function onMaximizedAboutToChange(mode) {
                    if (mode !== 0) {
                        if (tracker.plainState()) tracker.snapshotNormal();
                        tracker.freeze = true;
                        if (tracker.win.tile) tracker.savedTile = tracker.win.tile;
                        tracker.backToTile = false;
                    } else if (tracker.savedTile) {
                        tracker.backToTile = true;
                    }
                }
                function onMaximizedChanged() {
                    const w = tracker.win;
                    if (w.maximizeMode === 0) tracker.freeze = false;
                    if (tracker.backToTile && tracker.savedTile && root.cfg.zone_after_unmaximize) {
                        const t = tracker.savedTile;
                        tracker.savedTile = null;
                        tracker.backToTile = false;
                        try { t.manage(w); } catch (e) {}
                    }
                    tracker.changed();
                }
            }

            Component.onCompleted: snapshotNormal()
        }
    }

    function track(w) {
        if (!w || !w.normalWindow) return null;
        const id = wid(w);
        if (trackers[id]) return trackers[id];
        const t = trackerComponent.createObject(root, { win: w });
        trackers[id] = t;
        return t;
    }

    Connections {
        target: Workspace
        function onWindowAdded(w) {
            if (!root.track(w) || !root.managed(w) || !root.connected) return;
            root.call("Opened", [JSON.stringify(root.info(w))], function (ret) {
                if (!ret) return;
                try { root.startRestore(w, JSON.parse(ret)); } catch (e) { console.log("tessera: " + e); }
            });
        }
        function onWindowRemoved(w) {
            const id = root.wid(w);
            if (root.connected && root.remembered(w)) {
                try { root.call("Closed", [JSON.stringify(root.info(w))]); } catch (e) { console.log("tessera: " + e); }
            }
            const t = root.trackers[id];
            if (t) {
                delete root.trackers[id];
                t.destroy();
            }
        }
        function onScreensChanged() { root.sendTopology(); }
        function onDesktopsChanged() { root.sendTopology(); }
    }

    function sendTopology() {
        if (!connected) return;
        const screens = [];
        const ss = Workspace.screens;
        for (let i = 0; i < ss.length; i++) {
            const g = ss[i].geometry;
            screens.push({ name: ss[i].name, x: g.x, y: g.y, w: g.width, h: g.height,
                           manufacturer: ss[i].manufacturer || "", model: ss[i].model || "" });
        }
        const desktops = [];
        const ds = Workspace.desktops;
        for (let i = 0; i < ds.length; i++) desktops.push({ id: ds[i].id, name: ds[i].name });
        call("Topology", [JSON.stringify({ screens: screens, desktops: desktops,
                                           current_desktop: Workspace.currentDesktop.id })]);
    }

    // -------------------------------------------------------- shortcuts --
    // All of them can be changed under System Settings > Shortcuts > KWin.

    ShortcutHandler {
        name: "Tessera: Open"
        text: "Tessera: Open"
        sequence: "Meta+Ctrl+Alt+T"
        onActivated: root.call("Shortcut", ["open"])
    }
    ShortcutHandler {
        name: "Tessera: Save arrangement"
        text: "Tessera: Save arrangement"
        sequence: "Meta+Ctrl+Alt+S"
        onActivated: root.call("Shortcut", ["save"])
    }
    ShortcutHandler {
        name: "Tessera: Restore arrangement"
        text: "Tessera: Restore arrangement"
        sequence: "Meta+Ctrl+Alt+R"
        onActivated: root.call("Shortcut", ["restore"])
    }
    ShortcutHandler {
        name: "Tessera: Pause restoring"
        text: "Tessera: Pause or resume restoring windows"
        sequence: ""
        onActivated: root.call("Shortcut", ["pause"])
    }
    ShortcutHandler {
        name: "Tessera: Next zone"
        text: "Tessera: Move window to the next zone"
        sequence: "Meta+Ctrl+Alt+Right"
        onActivated: root.zoneTarget(1, false)
    }
    ShortcutHandler {
        name: "Tessera: Previous zone"
        text: "Tessera: Move window to the previous zone"
        sequence: "Meta+Ctrl+Alt+Left"
        onActivated: root.zoneTarget(-1, false)
    }
    ShortcutHandler {
        name: "Tessera: Release from zone"
        text: "Tessera: Take window out of its zone"
        sequence: "Meta+Ctrl+Alt+0"
        onActivated: {
            const w = Workspace.activeWindow;
            if (w && w.tile) w.tile.unmanage(w);
        }
    }
    Instantiator {
        model: 9
        delegate: ShortcutHandler {
            required property int index
            name: "Tessera: Zone " + (index + 1)
            text: "Tessera: Move window to zone " + (index + 1)
            sequence: "Meta+Ctrl+Alt+" + (index + 1)
            onActivated: root.zoneTarget(index + 1, true)
        }
    }

    Component.onCompleted: {
        const all = Workspace.windows;
        for (let i = 0; i < all.length; i++) track(all[i]);
        hello();
    }
}
