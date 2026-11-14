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
            syncTiling();
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
            syncTiling();
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

    // ----------------------------------------------------------- tiling --
    // Optional automatic tiling, off unless turned on in the settings. Each
    // screen and desktop has a binary tree of windows. A new window takes half
    // of the window that had focus, split along its longer side ("dwindle").
    // Leaves are `{ w: id }`, splits `{ a, b, ratio, dir }` with dir 0 =
    // along the longer side, 1 = side by side, 2 = on top of each other.

    property var tileTrees: ({})   // key -> root node
    property var tiledIn: ({})     // window id -> key
    property var lastFocus: ({})   // key -> window id
    property bool tilingBusy: false

    function tilingOn() {
        return cfg.tiling === true;
    }

    function wsKey(w) {
        return (w.output ? w.output.name : "") + "\n" + deskOf(w).id;
    }

    function globMatch(p, t) {
        const re = new RegExp("^" + p.replace(/[.+?^${}()|[\]\\]/g, "\\$&").replace(/\*/g, ".*") + "$", "i");
        return re.test(t);
    }

    function tileable(w) {
        if (!managed(w) || !w.moveable || !w.resizeable) return false;
        if (w.onAllDesktops || w.desktops.length !== 1 || w.tile) return false;
        const tr = trackers[wid(w)];
        if (tr && tr.floating) return false;
        // Fixed-size windows (most dialogs, splash screens) float.
        const min = w.minSize, max = w.maxSize;
        if (min && max && max.width > 0 && max.height > 0
            && max.width <= min.width && max.height <= min.height) return false;
        const cls = String(w.resourceClass).toLowerCase();
        const patterns = cfg["float"] || [];
        for (let i = 0; i < patterns.length; i++) {
            if (globMatch(patterns[i], cls)) return false;
        }
        return true;
    }

    function isLeaf(n) {
        return n.w !== undefined;
    }

    function findLeaf(n, id) {
        if (!n || id === undefined) return null;
        if (isLeaf(n)) return n.w === id ? n : null;
        return findLeaf(n.a, id) || findLeaf(n.b, id);
    }

    function lastLeaf(n) {
        return isLeaf(n) ? n : lastLeaf(n.b);
    }

    // The splits from the root down to the window, with the side it is on.
    function pathTo(n, id, out) {
        if (isLeaf(n)) return n.w === id ? out : null;
        out.push({ node: n, inA: true });
        if (pathTo(n.a, id, out)) return out;
        out[out.length - 1].inA = false;
        if (pathTo(n.b, id, out)) return out;
        out.pop();
        return null;
    }

    function tileInsert(w, under) {
        const id = wid(w);
        if (tiledIn[id]) return;
        const tr = trackers[id];
        if (tr && !tr.preTile) {
            const g = w.frameGeometry;
            tr.preTile = { x: g.x, y: g.y, width: g.width, height: g.height };
        }
        const key = wsKey(w);
        const leaf = { w: id };
        const top = tileTrees[key];
        if (!top) {
            tileTrees[key] = leaf;
        } else {
            const target = findLeaf(top, under) || findLeaf(top, lastFocus[key]) || lastLeaf(top);
            const old = { w: target.w };
            delete target.w;
            target.a = old;
            target.b = leaf;
            target.ratio = 0.5;
            target.dir = 0;
        }
        tiledIn[id] = key;
        relayout(key);
    }

    function tileRemove(id) {
        const key = tiledIn[id];
        if (!key) return;
        delete tiledIn[id];
        if (lastFocus[key] === id) delete lastFocus[key];
        const top = tileTrees[key];
        if (isLeaf(top)) {
            delete tileTrees[key];
            return;
        }
        const path = pathTo(top, id, []);
        if (!path || !path.length) return;
        const step = path[path.length - 1];
        const p = step.node;
        const sibling = step.inA ? p.b : p.a;
        delete p.a; delete p.b; delete p.ratio; delete p.dir; delete p.side; delete p.rect;
        for (const k in sibling) p[k] = sibling[k];
        relayout(key);
    }

    function shown(n) {
        if (isLeaf(n)) {
            const w = windowById(n.w);
            return !!w && !w.deleted && !w.minimized;
        }
        return shown(n.a) || shown(n.b);
    }

    function place(n, r, gap, out) {
        if (isLeaf(n)) {
            out.push({ id: n.w, r: r });
            return;
        }
        const sa = shown(n.a), sb = shown(n.b);
        if (!sa && !sb) return;
        if (!sb) { place(n.a, r, gap, out); return; }
        if (!sa) { place(n.b, r, gap, out); return; }
        const side = n.dir === 1 ? true : n.dir === 2 ? false : r.width >= r.height;
        n.side = side;
        n.rect = r;
        let ra, rb;
        if (side) {
            const wa = Math.round((r.width - gap) * n.ratio);
            ra = { x: r.x, y: r.y, width: wa, height: r.height };
            rb = { x: r.x + wa + gap, y: r.y, width: r.width - wa - gap, height: r.height };
        } else {
            const ha = Math.round((r.height - gap) * n.ratio);
            ra = { x: r.x, y: r.y, width: r.width, height: ha };
            rb = { x: r.x, y: r.y + ha + gap, width: r.width, height: r.height - ha - gap };
        }
        place(n.a, ra, gap, out);
        place(n.b, rb, gap, out);
    }

    function relayout(key) {
        const top = tileTrees[key];
        if (!top) return;
        const parts = key.split("\n");
        const screen = screenByName(parts[0]);
        const desk = desktopById(parts[1]);
        if (!screen || !desk) return;
        const area = Workspace.clientArea(KWin.MaximizeArea, screen, desk);
        const o = Math.max(0, cfg.tiling_outer_gap || 0);
        const gap = Math.max(0, cfg.tiling_gap || 0);
        const out = [];
        place(top, { x: area.x + o, y: area.y + o, width: area.width - 2 * o, height: area.height - 2 * o }, gap, out);
        tilingBusy = true;
        for (let i = 0; i < out.length; i++) {
            const w = windowById(out[i].id);
            const tr = trackers[out[i].id];
            if (!w || w.deleted || w.maximizeMode !== 0 || w.fullScreen || (tr && tr.userMoving)) continue;
            const r = out[i].r;
            const want = Qt.rect(Math.round(r.x), Math.round(r.y), Math.round(r.width), Math.round(r.height));
            if (tr) {
                const t = tr.tileTarget;
                if (!t || t.x !== want.x || t.y !== want.y || t.width !== want.width || t.height !== want.height)
                    tr.insist = null;
                tr.tileTarget = want;
            }
            const g = w.frameGeometry;
            if (Math.abs(g.x - want.x) > 1 || Math.abs(g.y - want.y) > 1
                || Math.abs(g.width - want.width) > 1 || Math.abs(g.height - want.height) > 1)
                w.frameGeometry = want;
        }
        tilingBusy = false;
    }

    function relayoutAll() {
        for (const key in tileTrees) relayout(key);
    }

    property var dirtyKeys: ({})
    Timer {
        id: relayoutTimer
        interval: 60
        onTriggered: {
            const keys = root.dirtyKeys;
            root.dirtyKeys = {};
            for (const key in keys) root.relayout(key);
        }
    }

    function scheduleRelayout(key) {
        if (!key) return;
        dirtyKeys[key] = true;
        relayoutTimer.restart();
    }

    // Brings the tiling in line with the settings and the windows: adds what
    // should be tiled, drops what should not, then places everything.
    function syncTiling() {
        if (!tilingOn()) {
            untileAll();
            return;
        }
        for (const id in tiledIn) {
            const w = windowById(id);
            if (!w || !tileable(w) || wsKey(w) !== tiledIn[id]) tileRemove(id);
        }
        const all = Workspace.windows;
        for (let i = 0; i < all.length; i++) {
            const w = all[i];
            if (!tiledIn[wid(w)] && track(w) && tileable(w)) tileInsert(w);
        }
        relayoutAll();
    }

    // Tiling turned off: windows go back to the size and place they had.
    function untileAll() {
        for (const id in tiledIn) {
            const w = windowById(id);
            const tr = trackers[id];
            if (w && !w.deleted && tr && tr.preTile && w.maximizeMode === 0 && !w.fullScreen) {
                const area = Workspace.clientArea(KWin.MaximizeArea, w.output, deskOf(w));
                const r = clampRect(tr.preTile, area);
                w.frameGeometry = Qt.rect(r.x, r.y, r.width, r.height);
            }
            if (tr) tr.preTile = null;
        }
        tileTrees = {};
        tiledIn = {};
        lastFocus = {};
    }

    function tiledWindowAt(key, pos, except) {
        for (const id in tiledIn) {
            if (tiledIn[id] !== key || id === except) continue;
            const w = windowById(id);
            if (!w || w.minimized) continue;
            const g = w.frameGeometry;
            if (pos.x >= g.x && pos.x < g.x + g.width && pos.y >= g.y && pos.y < g.y + g.height) return id;
        }
        return null;
    }

    function swapTiles(key, a, b) {
        const la = findLeaf(tileTrees[key], a), lb = findLeaf(tileTrees[key], b);
        if (!la || !lb) return;
        la.w = b;
        lb.w = a;
        relayout(key);
    }

    // Moved by hand: dropped on another tiled window swaps the two, dropped
    // on another screen or desktop joins the tiling there, anything else
    // snaps back. Resized by hand: the splits next to the moved edges follow.
    function tileMovedByHand(w, start) {
        const id = wid(w);
        const key = tiledIn[id];
        if (!key) return;
        const now = wsKey(w);
        if (now !== key) {
            const under = tiledWindowAt(now, Workspace.cursorPos, id);
            tileRemove(id);
            if (tileable(w)) tileInsert(w, under);
            return;
        }
        const g = w.frameGeometry;
        const resized = start && (Math.abs(g.width - start.width) > 2 || Math.abs(g.height - start.height) > 2);
        if (!resized) {
            const other = tiledWindowAt(key, Workspace.cursorPos, id);
            if (other) swapTiles(key, id, other);
            else relayout(key);
            return;
        }
        const path = pathTo(tileTrees[key], id, []) || [];
        const gap = Math.max(0, cfg.tiling_gap || 0);
        const edges = [
            { moved: Math.abs(g.x + g.width - (start.x + start.width)) > 2, side: true, inA: true },
            { moved: Math.abs(g.x - start.x) > 2, side: true, inA: false },
            { moved: Math.abs(g.y + g.height - (start.y + start.height)) > 2, side: false, inA: true },
            { moved: Math.abs(g.y - start.y) > 2, side: false, inA: false }
        ];
        for (let e = 0; e < edges.length; e++) {
            if (!edges[e].moved) continue;
            for (let i = path.length - 1; i >= 0; i--) {
                const n = path[i].node;
                if (!n.rect || n.side !== edges[e].side || path[i].inA !== edges[e].inA) continue;
                const r = n.rect;
                const total = (n.side ? r.width : r.height) - gap;
                let first;
                if (n.side) first = edges[e].inA ? g.x + g.width - r.x : g.x - gap - r.x;
                else first = edges[e].inA ? g.y + g.height - r.y : g.y - gap - r.y;
                if (total > 0) n.ratio = Math.max(0.1, Math.min(0.9, first / total));
                break;
            }
        }
        relayout(key);
    }

    function tileNeighbour(direction) {
        const w = Workspace.activeWindow;
        if (!w) return;
        const id = wid(w);
        const key = tiledIn[id];
        if (!key) return;
        const g = w.frameGeometry;
        const cx = g.x + g.width / 2, cy = g.y + g.height / 2;
        let best = null, bestScore = Infinity;
        for (const other in tiledIn) {
            if (other === id || tiledIn[other] !== key) continue;
            const o = windowById(other);
            if (!o || o.minimized) continue;
            const og = o.frameGeometry;
            const dx = og.x + og.width / 2 - cx, dy = og.y + og.height / 2 - cy;
            const along = direction === "left" ? -dx : direction === "right" ? dx : direction === "up" ? -dy : dy;
            const across = (direction === "left" || direction === "right") ? Math.abs(dy) : Math.abs(dx);
            if (along <= 1) continue;
            const score = along + 2 * across;
            if (score < bestScore) { bestScore = score; best = other; }
        }
        if (best) swapTiles(key, id, best);
    }

    function toggleFloating() {
        const w = Workspace.activeWindow;
        if (!w || !tilingOn()) return;
        const tr = track(w);
        if (!tr) return;
        const id = wid(w);
        if (tiledIn[id]) {
            tr.floating = true;
            tileRemove(id);
            const area = Workspace.clientArea(KWin.MaximizeArea, w.output, deskOf(w));
            const size = tr.preTile || { width: area.width / 2, height: area.height / 2 };
            const width = Math.min(size.width, area.width), height = Math.min(size.height, area.height);
            w.frameGeometry = Qt.rect(Math.round(area.x + (area.width - width) / 2),
                                      Math.round(area.y + (area.height - height) / 2), width, height);
            tr.preTile = null;
        } else {
            tr.floating = false;
            if (w.maximizeMode !== 0) w.setMaximize(false, false);
            if (tileable(w)) tileInsert(w);
        }
    }

    function toggleSplit() {
        const w = Workspace.activeWindow;
        if (!w) return;
        const id = wid(w);
        const key = tiledIn[id];
        if (!key) return;
        const path = pathTo(tileTrees[key], id, []);
        if (!path || !path.length) return;
        const n = path[path.length - 1].node;
        n.dir = n.side === false ? 1 : 2;
        relayout(key);
    }

    // A tiled window changed by itself or through KWin.
    function tiledWindowChanged(tr) {
        const w = tr.win;
        const id = wid(w);
        const key = tiledIn[id];
        if (!key || tr.userMoving) return;
        if (!tileable(w) || wsKey(w) !== key) {
            // Into a zone, onto all desktops, to another screen or desktop.
            tileRemove(id);
            if (tilingOn() && tileable(w)) tileInsert(w);
            return;
        }
        if (tilingBusy || w.maximizeMode !== 0 || w.fullScreen) return;
        const g = w.frameGeometry, t = tr.tileTarget;
        if (t && Math.abs(g.x - t.x) <= 1 && Math.abs(g.y - t.y) <= 1
            && Math.abs(g.width - t.width) <= 1 && Math.abs(g.height - t.height) <= 1) return;
        // A window that keeps answering with the same size (a minimum size
        // larger than its tile) is left at that size instead of fought.
        const now = { x: g.x, y: g.y, width: g.width, height: g.height };
        const last = tr.insist;
        if (last && last.width === now.width && last.height === now.height) return;
        tr.insist = now;
        scheduleRelayout(key);
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
            // Automatic tiling: taken out by hand, the size before tiling,
            // where the tiling last put it, and a size it keeps insisting on.
            property bool floating: false
            property var preTile: null
            property var tileTarget: null
            property var insist: null
            property var tileMoveStart: null

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
                if (root.tiledIn[root.wid(win)]) root.tiledWindowChanged(tracker);
            }

            function tilingStateChanged() {
                const key = root.tiledIn[root.wid(win)];
                if (key) root.scheduleRelayout(key);
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
                function onFullScreenChanged() { tracker.changed(); tracker.tilingStateChanged(); }
                function onDesktopsChanged() { tracker.changed(); }
                function onOutputChanged() { tracker.changed(); }
                function onMinimizedChanged() { tracker.changed(); tracker.tilingStateChanged(); }
                function onKeepAboveChanged() { tracker.changed(); }
                function onInteractiveMoveResizeStarted() {
                    tracker.userMoving = true;
                    const g = tracker.win.frameGeometry;
                    tracker.tileMoveStart = { x: g.x, y: g.y, width: g.width, height: g.height };
                    tracker.moveStart = tracker.plainState()
                        ? { x: g.x, y: g.y, width: g.width, height: g.height } : null;
                }
                function onInteractiveMoveResizeFinished() {
                    tracker.userMoving = false;
                    // A tiled window moved by hand stays tiled; its place or
                    // size is not remembered as set by hand.
                    if (root.tiledIn[root.wid(tracker.win)]) {
                        tracker.moveStart = null;
                        root.tileMovedByHand(tracker.win, tracker.tileMoveStart);
                        tracker.changed();
                        return;
                    }
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
                    tracker.tilingStateChanged();
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
            if (root.tilingOn() && root.tileable(w)) root.tileInsert(w);
            root.call("Opened", [JSON.stringify(root.info(w))], function (ret) {
                if (!ret) return;
                try {
                    let r = JSON.parse(ret);
                    // A tiled window keeps only its desktop; the tiling
                    // decides where it goes and how large it is.
                    if (root.tiledIn[root.wid(w)])
                        r = { placement: r.placement, desktop: r.desktop };
                    root.startRestore(w, r);
                } catch (e) { console.log("tessera: " + e); }
            });
        }
        function onWindowActivated(w) {
            if (!w) return;
            const id = root.wid(w);
            const key = root.tiledIn[id];
            if (key) root.lastFocus[key] = id;
        }
        function onWindowRemoved(w) {
            const id = root.wid(w);
            root.tileRemove(id);
            if (root.connected && root.remembered(w)) {
                try { root.call("Closed", [JSON.stringify(root.info(w))]); } catch (e) { console.log("tessera: " + e); }
            }
            const t = root.trackers[id];
            if (t) {
                delete root.trackers[id];
                t.destroy();
            }
        }
        function onScreensChanged() { root.sendTopology(); root.syncTiling(); }
        function onDesktopsChanged() { root.sendTopology(); root.syncTiling(); }
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
    ShortcutHandler {
        name: "Tessera: Toggle tiling"
        text: "Tessera: Turn automatic tiling on or off"
        sequence: "Meta+Ctrl+Alt+A"
        onActivated: root.call("Shortcut", ["tiling"])
    }
    ShortcutHandler {
        name: "Tessera: Float window"
        text: "Tessera: Take the window out of the tiling or back in"
        sequence: "Meta+Ctrl+Alt+F"
        onActivated: root.toggleFloating()
    }
    ShortcutHandler {
        name: "Tessera: Toggle split"
        text: "Tessera: Split the window's tile the other way"
        sequence: "Meta+Ctrl+Alt+J"
        onActivated: root.toggleSplit()
    }
    Instantiator {
        model: [["left", "Left"], ["right", "Right"], ["up", "Up"], ["down", "Down"]]
        delegate: ShortcutHandler {
            required property var modelData
            name: "Tessera: Swap " + modelData[0]
            text: "Tessera: Swap the window with the tiled window " + (modelData[0] === "up" ? "above"
                  : modelData[0] === "down" ? "below" : "to the " + modelData[0])
            sequence: "Meta+Shift+Alt+" + modelData[1]
            onActivated: root.tileNeighbour(modelData[0])
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
