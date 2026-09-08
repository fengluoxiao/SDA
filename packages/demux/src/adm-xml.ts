import { DOMImplementation, type Element } from "@xmldom/xmldom";
import { SaxesParser } from "saxes";

/** Retain the small reference graph, or one block at a time on the second pass. */
export async function scanAdmXml(source: AsyncIterable<Uint8Array>, consume?: (channelId: string, block: Element) => void) {
  const document = new DOMImplementation().createDocument(null, "");
  const parser = new SaxesParser({ xmlns: true });
  const stack: { local: string; element?: Element; channelId: string }[] = [];
  let blockDepth = 0, graphSize = 0, blockSize = 0, lastEvent = 0, padded = false;
  const fail = (message: string): never => { throw new Error(`ADM: ${message}`); };
  const mark = () => { lastEvent = parser.position; };
  const budget = (size: number) => {
    if (blockDepth) {
      blockSize += size;
      if (blockSize > 1024 * 1024) fail("individual audio block exceeds XML resource budget");
    } else {
      graphSize += size;
      if (graphSize > 16 * 1024 * 1024) fail("reference graph exceeds XML resource budget");
    }
  };
  parser.on("error", error => fail(`malformed XML: ${error.message}`));
  parser.on("doctype", () => fail("XML document types are unsupported"));
  parser.on("comment", mark);
  parser.on("processinginstruction", mark);
  parser.on("xmldecl", mark);
  parser.on("opentag", tag => {
    mark();
    if (stack.length >= 256) fail("XML nesting exceeds resource budget");
    const parent = stack.at(-1);
    const isBlock = tag.local === "audioBlockFormat" && parent?.local === "audioChannelFormat";
    if (isBlock) { blockDepth = stack.length + 1; blockSize = 0; }
    const retain = consume ? blockDepth > 0 : blockDepth === 0;
    let element: Element | undefined;
    if (retain) {
      budget(128 + tag.name.length * 2);
      element = document.createElementNS(tag.uri || null, tag.name);
      for (const attribute of Object.values(tag.attributes)) {
        budget(64 + (attribute.name.length + attribute.value.length) * 2);
        element.setAttributeNS(attribute.uri || null, attribute.name, attribute.value);
      }
      if (parent?.element && !isBlock) parent.element.appendChild(element);
      else if (!consume && !parent) document.appendChild(element);
    }
    const channelId = tag.local === "audioChannelFormat"
      ? Object.values(tag.attributes).find(attribute => attribute.name === "audioChannelFormatID")?.value ?? ""
      : parent?.channelId ?? "";
    stack.push({ local: tag.local, element, channelId });
  });
  const onText = (text: string) => {
    mark();
    const element = stack.at(-1)?.element;
    if (element && text.trim()) {
      budget(32 + text.length * 2);
      element.appendChild(document.createTextNode(text));
    }
  };
  parser.on("text", onText);
  parser.on("cdata", onText);
  parser.on("closetag", () => {
    mark();
    const entry = stack.pop()!;
    if (blockDepth === stack.length + 1) {
      if (consume && entry.element) consume(entry.channelId, entry.element);
      blockDepth = 0;
    }
  });
  const decoder = new TextDecoder("utf-8", { fatal: true });
  const write = (text: string) => {
    if (padded && /[^\0]/.test(text)) fail("non-padding data after XML NUL padding");
    if (text.endsWith("\0")) { padded = true; text = text.replace(/\0+$/, ""); }
    parser.write(text);
    if (parser.position - lastEvent > 1024 * 1024) fail("XML token exceeds resource budget");
  };
  for await (const bytes of source) {
    for (let offset = 0; offset < bytes.length; offset += 64 * 1024) {
      write(decoder.decode(bytes.subarray(offset, offset + 64 * 1024), { stream: true }));
    }
  }
  write(decoder.decode());
  parser.close();
  return document;
}
