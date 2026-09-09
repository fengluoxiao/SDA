const fs = require("node:fs/promises");
const path = require("node:path");

function createMediaBrowser({ app, readSettings, writeSettings, isMediaFile }) {
  const checkedPath = value => {
    if (typeof value !== "string" || !path.isAbsolute(value) || value.includes("\0")) throw new Error("目录路径无效");
    return path.normalize(value);
  };
  const same = (a, b) => process.platform === "win32" ? a.toLowerCase() === b.toLowerCase() : a === b;
  const saved = () => {
    const settings = readSettings();
    return {
      recent: Array.isArray(settings.mediaBrowserRecent) ? settings.mediaBrowserRecent.filter(x => typeof x === "string").slice(0, 12) : [],
      favorites: Array.isArray(settings.mediaBrowserFavorites) ? settings.mediaBrowserFavorites.filter(x => typeof x === "string").slice(0, 100) : [],
    };
  };
  const remember = directory => {
    const recent = [directory, ...saved().recent.filter(p => !same(p, directory))].slice(0, 12);
    writeSettings({ lastMediaDirectory: directory, mediaBrowserRecent: recent });
  };
  return async function browse(action, value) {
    if (action === "places") {
      const places = ["home", "music", "downloads", "documents", "desktop"].map(key => ({
        name: {home:"主目录",music:"音乐",downloads:"下载",documents:"文档",desktop:"桌面"}[key], path: app.getPath(key),
      }));
      const roots = process.platform === "win32"
        ? (await Promise.all(Array.from({length:26}, async (_, i) => {
          const root = String.fromCharCode(65 + i) + ":\\";
          try { await fs.access(root); return {name:root,path:root}; } catch { return null; }
        }))).filter(Boolean) : [{name:"/",path:"/"}];
      return {...saved(), places:[...places,...roots], initial:readSettings().lastMediaDirectory || app.getPath("music")};
    }
    if (action === "list") {
      const directory = checkedPath(value);
      const entries = (await fs.readdir(directory, {withFileTypes:true}))
        .filter(e => e.isDirectory() || (e.isFile() && isMediaFile(e.name)))
        .map(e => ({name:e.name,path:path.join(directory,e.name),directory:e.isDirectory()}))
        .sort((a,b) => Number(b.directory)-Number(a.directory) || a.name.localeCompare(b.name, undefined, {numeric:true}));
      return {path:directory,parent:path.dirname(directory),entries};
    }
    if (action === "favorite" || action === "unfavorite" || action === "forget") {
      const directory = checkedPath(value);
      const state = saved();
      if (action === "favorite") {
        if (!(await fs.stat(directory)).isDirectory()) throw new Error("只能收藏目录");
        state.favorites = [...state.favorites.filter(p => !same(p,directory)),directory].slice(-100);
      } else if (action === "unfavorite") state.favorites = state.favorites.filter(p => !same(p,directory));
      else state.recent = state.recent.filter(p => !same(p,directory));
      writeSettings({mediaBrowserFavorites:state.favorites,mediaBrowserRecent:state.recent});
      return state;
    }
    if (action === "files") {
      if (!Array.isArray(value) || !value.length || value.length > 10000) throw new Error("请选择媒体文件");
      const paths = [...new Set(value.map(checkedPath))];
      for (const file of paths) if (!isMediaFile(file) || !(await fs.stat(file)).isFile()) throw new Error("文件不存在或格式不支持");
      remember(path.dirname(paths[0]));
      return paths;
    }
    if (action === "folder") {
      const directory = checkedPath(value), files = [], pending = [directory];
      let count = 0;
      while (pending.length) {
        if (++count > 20000 || files.length > 10000) throw new Error("目录过大，请选择更具体的子目录");
        const current = pending.pop();
        let entries;
        try { entries = await fs.readdir(current, {withFileTypes:true}); }
        catch (error) { if (current === directory) throw error; else continue; }
        entries.sort((a,b)=>a.name.localeCompare(b.name,undefined,{numeric:true}));
        for (const e of entries) {
          const target = path.join(current,e.name);
          if (e.isDirectory()) pending.push(target);
          else if (e.isFile() && isMediaFile(e.name)) files.push(target);
        }
      }
      if (!files.length) throw new Error("此目录中没有支持的媒体文件");
      if (files.length > 10000) throw new Error("目录内文件过多，请选择子目录");
      remember(directory);
      return files.sort((a,b)=>a.localeCompare(b,undefined,{numeric:true}));
    }
    throw new Error("不支持的文件浏览操作");
  };
}
module.exports = {createMediaBrowser};
