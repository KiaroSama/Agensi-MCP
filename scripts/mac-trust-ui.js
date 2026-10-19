$.NSFileHandle.fileHandleWithStandardOutput.writeData($('P1\n').dataUsingEncoding($.NSUTF8StringEncoding));
ObjC.import('Foundation');
ObjC.import('stdlib');
function run() {
    const args = $.NSProcessInfo.processInfo.arguments;
    const entry = $(String(ObjC.unwrap(args.objectAtIndex(3)))).stringByStandardizingPath;
    function load(name) {
        const path = entry.stringByDeletingLastPathComponent.stringByAppendingPathComponent(name);
        if (String(ObjC.unwrap(path.stringByResolvingSymlinksInPath)) !== String(ObjC.unwrap(path))) $.exit(64);
        const error = Ref();
        const source = $.NSString.stringWithContentsOfFileEncodingError(path, $.NSUTF8StringEncoding, error);
        if (!source) $.exit(64);
        return Function(String(ObjC.unwrap(source)))();
    }
    const channel = load('mac-input-channel.js')();
    $.NSFileHandle.fileHandleWithStandardOutput.writeData($('P2\n').dataUsingEncoding($.NSUTF8StringEncoding));
    load('mac-input-core.js')(false, channel);
}
