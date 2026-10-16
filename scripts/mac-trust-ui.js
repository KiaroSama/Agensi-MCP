ObjC.import('Foundation');
ObjC.import('stdlib');

let stage = 1;
function fail(code) {
    $.NSFileHandle.fileHandleWithStandardOutput.writeData($('E' + stage + ':' + code).dataUsingEncoding($.NSUTF8StringEncoding));
    $.exit(code > 0 && code < 128 ? code : 70);
}
function agentWindows(system) {
    const agents = system.applicationProcesses.whose({ name: 'SecurityAgent' })();
    if (agents.length > 1) fail(65);
    if (agents.length === 0) return [];
    if (agents[0].bundleIdentifier() !== 'com.apple.SecurityAgent') fail(65);
    return agents[0].windows();
}
function inspect(window) {
    const pending = [{ element: window, depth: 0 }];
    const result = { text: [], users: [], passwords: [], buttons: [] };
    let count = 0;
    while (pending.length) {
        const entry = pending.pop();
        if (++count > 160 || entry.depth > 8) fail(66);
        const element = entry.element;
        const role = element.role();
        if (role === 'AXStaticText') result.text.push(String(element.value()));
        if (role === 'AXTextField') {
            if (element.subrole() === 'AXSecureTextField') result.passwords.push(element);
            else result.users.push(element);
        }
        if (role === 'AXButton' && element.enabled()) result.buttons.push(element);
        for (const child of element.uiElements()) pending.push({ element: child, depth: entry.depth + 1 });
    }
    return result;
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
        const system = Application('System Events');
        if (!system.uiElementsEnabled()) fail(65);
        stage = 3;
        if (agentWindows(system).length !== 0) fail(65);
        stage = 4;
        $.NSFileHandle.fileHandleWithStandardOutput.writeData($('R').dataUsingEncoding($.NSUTF8StringEncoding));
        const deadline = Date.now() + 4000;
        while (Date.now() < deadline) {
            const windows = agentWindows(system);
            if (windows.length > 1) fail(65);
            if (windows.length === 1) {
                const controls = inspect(windows[0]);
                if (!controls.text.some(text => text.indexOf(input.marker) !== -1)) fail(67);
                const approve = controls.buttons.filter(button => ['OK', 'Modify Settings', 'Update Settings'].indexOf(button.name()) !== -1);
                if (controls.users.length !== 1 || controls.passwords.length !== 1 || approve.length !== 1) fail(68);
                controls.users[0].value = input.name;
                controls.passwords[0].value = input.password;
                input.password = '';
                approve[0].click();
                return;
            }
            delay(0.05);
        }
        fail(69);
    } catch (error) {
        const code = Number(error.errorNumber);
        fail(Number.isInteger(code) && Math.abs(code) <= 1000000 ? code : 70);
    }
}
