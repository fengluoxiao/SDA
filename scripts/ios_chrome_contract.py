"""Check the mounted system controls in a real native-view inspection report."""
def ready(data,stage):
 views=data.get("nativeViews",[]);classes=[v["class"]for v in views]
 hosting=any("Hosting"in c for c in classes);tabs=any("TabBar"in c for c in classes)
 form=any("CollectionView"in c or "TableView"in c for c in classes);navigation=any("NavigationBar"in c for c in classes)
 result=hosting and (form and navigation if stage.startswith("settings")else tabs)
 if stage.startswith("settings"):
  result=result and any(b.get("title")=="设置"for b in data.get("navigationBars",[]))
  result=result and any(v.get("text")=="设置"and v.get("alpha",0)>0 for v in views)
  if stage in ("settings","settings-dark"):
   titles=[v for v in views if v.get("text")=="设置"and v.get("alpha",0)>0 and v.get("y",-1)>=0 and 0<v.get("height",0)<=30]
   rows=[v for v in views if v.get("class")=="ListCollectionViewCell"and v.get("y",-1)>=0 and v.get("width",0)>100]
   large=[v for v in views if v.get("text")=="设置"and v.get("alpha",0)>0 and v.get("y",-1)>=0 and v.get("height",0)>30]
   result=result and bool(titles)and not large and bool(rows)and min(v["y"]for v in rows)>=max(v["y"]+v["height"]for v in titles)-1
 if stage=="player":
  def mounted(name):return any(v.get("class")==name and v.get("alpha",0)>0 and v.get("width",0)>100 and v.get("height",0)>0 and v.get("y",-1)>=0 for v in views)
  native=mounted("SdaSystemVolumeView")and mounted("CenteredSystemVolumeView")
  fallback=any(v.get("label")=="音量滑块"for v in views)
  result=result and (native or fallback)
 if stage=="mini-player":result=result and any("TabsBottomAccessory"in c for c in classes)and any(v.get("label")=="展开正在播放"for v in views)
 return bool(result)
