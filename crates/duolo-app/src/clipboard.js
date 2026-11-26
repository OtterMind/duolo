// 读剪贴板里的文件或图片（macOS，由 platform.rs 用 osascript -l JavaScript 运行）。
// 复制的是文件（访达里 ⌘C）：输出每个文件的路径，一行一个。剪贴板附带的图像只是文件图标，不用它。
// 复制的是图片（截图到剪贴板、网页上的图片）：存成 PNG 写到 argv[0]，输出这个路径。都没有时输出空。
ObjC.import("AppKit");
function run(argv) {
  const pb = $.NSPasteboard.generalPasteboard;
  const opts = $.NSDictionary.dictionaryWithObjectForKey($.NSNumber.numberWithBool(true), $.NSPasteboardURLReadingFileURLsOnlyKey);
  const urls = pb.readObjectsForClassesOptions($.NSArray.arrayWithObject($.NSURL), opts);
  if (urls && !urls.isNil() && urls.count > 0) {
    const out = [];
    for (let i = 0; i < urls.count; i++) out.push(urls.objectAtIndex(i).path.js);
    return out.join("\n");
  }
  let data = pb.dataForType("public.png");
  if (!data || data.isNil()) {
    const tiff = pb.dataForType("public.tiff");
    if (tiff && !tiff.isNil()) data = $.NSBitmapImageRep.imageRepWithData(tiff).representationUsingTypeProperties(4, $());
  }
  if (data && !data.isNil() && data.writeToFileAtomically(argv[0], true)) return argv[0];
  return "";
}
