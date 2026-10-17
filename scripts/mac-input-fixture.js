ObjC.import('Foundation');
ObjC.import('stdlib');
function run() {
    const args = $.NSProcessInfo.processInfo.arguments;
    const entry = $(String(ObjC.unwrap(args.objectAtIndex(3)))).stringByStandardizingPath;
    const core = entry.stringByDeletingLastPathComponent.stringByAppendingPathComponent('mac-input-core.js');
    if (String(ObjC.unwrap(core.stringByResolvingSymlinksInPath)) !== String(ObjC.unwrap(core))) $.exit(64);
    const error = Ref();
    const source = $.NSString.stringWithContentsOfFileEncodingError(core, $.NSUTF8StringEncoding, error);
    if (!source) $.exit(64);
    Function(String(ObjC.unwrap(source)))()(true);
}
