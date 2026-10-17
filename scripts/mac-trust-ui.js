ObjC.import('Foundation');
ObjC.import('stdlib');
ObjC.import('ApplicationServices');
ObjC.import('AppKit');
ObjC.import('Carbon');

let stage = 1;
let agentPID = null;
let agentApplication = null;
let pairedInput = false;
function fail(code) {
    if (pairedInput) throw { errorNumber: code };
    $.NSFileHandle.fileHandleWithStandardOutput.writeData($('E' + stage + ':' + code + '\n').dataUsingEncoding($.NSUTF8StringEncoding));
    $.exit(code > 0 && code < 128 ? code : 70);
}
function attribute(element, name, optional) {
    const value = Ref();
    const status = Number($.AXUIElementCopyAttributeValue(element, $(name), value));
    if (optional && (status === -25205 || status === -25212)) return null;
    if (status === -25204 && stage === 4) throw { snapshotPending: true };
    if (status !== 0) fail(status);
    return ObjC.castRefToObject(value[0]);
}
function text(element, name, optional) {
    const value = attribute(element, name, optional);
    return value === null ? '' : String(ObjC.unwrap(value));
}
function children(element, name) {
    const value = attribute(element, name, true);
    if (value === null) return [];
    const count = Number(value.count);
    if (!Number.isInteger(count) || count < 0 || count > 160) fail(66);
    const result = [];
    for (let index = 0; index < count; index++) result.push(value.objectAtIndex(index));
    return result;
}
function agentWindows() {
    const agents = $.NSRunningApplication.runningApplicationsWithBundleIdentifier('com.apple.SecurityAgent');
    if (Number(agents.count) > 1) fail(65);
    if (Number(agents.count) === 0) return [];
    const pid = Number(agents.objectAtIndex(0).processIdentifier);
    if (agentPID !== null && agentPID !== pid) fail(65);
    agentPID = pid;
    if (agentApplication === null) agentApplication = agents.objectAtIndex(0);
    if (agentApplication.terminated) fail(65);
    const app = $.AXUIElementCreateApplication(pid);
    if (Number($.AXUIElementSetMessagingTimeout(app, 0.2)) !== 0) fail(66);
    return children(app, 'AXWindows');
}
function inspect(window, marker) {
    const pending = [{ element: window, depth: 0 }];
    const result = { window, text: [], nonces: [], users: [], passwords: [], buttons: [] };
    let count = 0;
    while (pending.length) {
        const entry = pending.pop();
        if (++count > 160 || entry.depth > 8) fail(66);
        const element = entry.element;
        const role = text(element, 'AXRole');
        if (role === 'AXStaticText') {
            const value = text(element, 'AXValue');
            result.text.push(value);
            if (marker && value.indexOf(marker) !== -1) result.nonces.push(element);
        }
        if (role === 'AXTextField') {
            if (text(element, 'AXSubrole', true) === 'AXSecureTextField') result.passwords.push(element);
            else result.users.push(element);
        }
        if (role === 'AXButton' && ObjC.unwrap(attribute(element, 'AXEnabled'))) result.buttons.push(element);
        for (const child of children(element, 'AXChildren')) pending.push({ element: child, depth: entry.depth + 1 });
    }
    return result;
}
let keyboardTraced = false;
function usKeyboard() {
    const trace = !keyboardTraced;
    keyboardTraced = true;
    function boundary(number) {
        if (trace) $.NSFileHandle.fileHandleWithStandardOutput.writeData(
            $('B' + number + '\n').dataUsingEncoding($.NSUTF8StringEncoding));
    }
    boundary(1);
    const input = $.TISCopyCurrentKeyboardInputSource();
    boundary(2);
    const layout = $.TISCopyCurrentKeyboardLayoutInputSource();
    boundary(3);
    try {
        return Boolean(input && layout) && [input, layout].every((source, index) => {
            boundary(4 + index);
            const identifier = $.TISGetInputSourceProperty(source, $.kTISPropertyInputSourceID);
            return identifier && String(ObjC.unwrap(ObjC.castRefToObject(identifier))) === 'com.apple.keylayout.US';
        });
    } finally {
        boundary(6);
        if (input) $.CFRelease(input);
        boundary(7);
        if (layout) $.CFRelease(layout);
        boundary(8);
    }
}
function focused(window, secure, nonce, marker, deadline) {
    if (Date.now() >= deadline || !usKeyboard()) return false;
    const windows = agentWindows();
    if (windows.length !== 1 || !$.CFEqual(windows[0], window)) return false;
    if (text(nonce, 'AXValue').indexOf(marker) === -1) return false;
    const app = $.AXUIElementCreateApplication(agentPID);
    if (Number($.AXUIElementSetMessagingTimeout(app, 0.2)) !== 0) fail(66);
    return Boolean($.CFEqual(attribute(app, 'AXFocusedWindow'), window)) &&
        Boolean($.CFEqual(attribute(app, 'AXFocusedUIElement'), secure));
}
function enterPassword(window, secure, nonce, input, deadline) {
    ObjC.bindFunction('CGEventPostToPid', ['void', ['int', 'void *']]);
    if (!usKeyboard()) fail(83);
    if (Number($.AXUIElementSetAttributeValue(secure, $('AXFocused'), $.kCFBooleanTrue)) !== 0) fail(76);
    while (!focused(window, secure, nonce, input.marker, deadline)) {
        if (Date.now() >= deadline) fail(76);
        delay(0.01);
    }
    const source = $.CGEventSourceCreate(-1);
    if (!source) fail(76);
    // Physical ANSI-US positions; secure input may ignore Unicode overrides.
    const keys = { '0': 0x1d, '1': 0x12, '2': 0x13, '3': 0x14, '4': 0x15,
        '5': 0x17, '6': 0x16, '7': 0x1a, '8': 0x1c, '9': 0x19,
        a: 0x00, b: 0x0b, c: 0x08, d: 0x02, e: 0x0e, f: 0x03 };
    for (let index = 0; index < input.password.length; index++) {
        const key = keys[input.password[index]];
        if (key === undefined) fail(64);
        const down = $.CGEventCreateKeyboardEvent(source, key, true);
        const up = $.CGEventCreateKeyboardEvent(source, key, false);
        if (!down || !up) fail(76);
        $.CGEventSetFlags(down, 0);
        $.CGEventSetFlags(up, 0);
        let sent = false;
        let released = false;
        pairedInput = true;
        try {
            if (!focused(window, secure, nonce, input.marker, deadline)) fail(76);
            $.CGEventPostToPid(agentPID, down);
            sent = true;
            if (!focused(window, secure, nonce, input.marker, deadline)) fail(77);
            $.CGEventPostToPid(agentPID, up);
            released = true;
        } finally {
            pairedInput = false;
            if (sent && !released && agentApplication && !agentApplication.terminated &&
                Number(agentApplication.processIdentifier) === agentPID) $.CGEventPostToPid(agentPID, up);
        }
    }
}
function run() {
    try {
        const data = $.NSFileHandle.fileHandleWithStandardInput.readDataOfLength(4097);
        if (Number(data.length) > 4096) fail(64);
        const decoded = $.NSString.alloc.initWithDataEncoding(data, $.NSUTF8StringEncoding);
        if (!decoded) fail(64);
        const input = JSON.parse(ObjC.unwrap(decoded));
        if (Object.keys(input).sort().join(',') !== 'marker,name,password' ||
            typeof input.marker !== 'string' || !/^mac-trust-[0-9a-f]{32}$/.test(input.marker) ||
            typeof input.name !== 'string' || !/^_agensitls[0-9a-f]{16}$/.test(input.name) ||
            typeof input.password !== 'string' || !/^[0-9a-f]{64}$/.test(input.password)) fail(64);
        stage = 2;
        if (!$.AXIsProcessTrusted()) fail(65);
        stage = 3;
        if (agentWindows().length !== 0) fail(65);
        stage = 4;
        $.NSFileHandle.fileHandleWithStandardOutput.writeData($('R\n').dataUsingEncoding($.NSUTF8StringEncoding));
        const deadline = Date.now() + 4000;
        while (Date.now() < deadline) {
            let controls;
            let approve;
            try {
                const windows = agentWindows();
                if (windows.length > 1) fail(65);
                if (windows.length === 1) {
                    controls = inspect(windows[0], input.marker);
                    if (controls.nonces.length === 0) fail(67);
                    approve = controls.buttons.filter(button => ['OK', 'Modify Settings', 'Update Settings'].indexOf(text(button, 'AXTitle')) !== -1);
                    if (controls.users.length !== 1 || controls.passwords.length !== 1 || approve.length !== 1) fail(68);
                    if (agentWindows().length !== 1) fail(65);
                }
            } catch (error) {
                if (!error.snapshotPending) throw error;
                controls = null;
                approve = null;
            }
            if (controls && approve) {
                if (Number($.AXUIElementSetAttributeValue(controls.users[0], $('AXValue'), $(input.name))) !== 0) fail(68);
                if (text(controls.users[0], 'AXValue') !== input.name) fail(74);
                enterPassword(controls.window, controls.passwords[0], controls.nonces[0], input, deadline);
                input.password = '';
                if (text(controls.users[0], 'AXValue') !== input.name) fail(78);
                if (!focused(controls.window, controls.passwords[0], controls.nonces[0], input.marker, deadline)) fail(76);
                if (Number($.AXUIElementPerformAction(approve[0], $('AXPress'))) !== 0) fail(68);
                while (Date.now() < deadline) {
                    const remaining = agentWindows();
                    if (remaining.length === 0) {
                        $.NSFileHandle.fileHandleWithStandardOutput.writeData($('E4:0\n').dataUsingEncoding($.NSUTF8StringEncoding));
                        return;
                    }
                    if (remaining.length !== 1) fail(65);
                    const observed = inspect(remaining[0]);
                    if (!observed.text.some(value => value.indexOf(input.marker) !== -1)) fail(67);
                    if (observed.users.length !== 1 || text(observed.users[0], 'AXValue') !== input.name) fail(79);
                    if (observed.text.some(value => /wrong password|incorrect password|try again|authentication failed/i.test(value))) fail(80);
                    delay(0.05);
                }
                fail(75);
            }
            delay(0.05);
        }
        fail(69);
    } catch (error) {
        const code = Number(error.errorNumber);
        fail(Number.isInteger(code) && Math.abs(code) <= 1000000 ? code : 70);
    }
}
