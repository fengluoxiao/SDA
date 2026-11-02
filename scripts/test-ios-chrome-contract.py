import unittest
from ios_chrome_contract import ready

def view(name,label="",width=283,height=44):
 return {"class":name,"label":label,"width":width,"height":height,"alpha":1,"y":711}
class Chrome(unittest.TestCase):
 def report(self):return {"nativeViews":[view("UIHostingView"),view("UITabBar"),view("SdaSystemVolumeView"),view("CenteredSystemVolumeView")]}
 def test_native_system_volume_is_accepted_without_fallback_label(self):self.assertTrue(ready(self.report(),"player"))
 def test_missing_volume_child_is_rejected(self):
  d=self.report();d["nativeViews"].pop();self.assertFalse(ready(d,"player"))
 def test_zero_size_volume_is_rejected(self):
  d=self.report();d["nativeViews"][-1]["width"]=0;self.assertFalse(ready(d,"player"))
 def test_missing_tab_bar_is_rejected(self):
  d=self.report();d["nativeViews"]=[v for v in d["nativeViews"]if v["class"]!="UITabBar"];self.assertFalse(ready(d,"player"))
 def test_approved_fallback_slider_is_still_accepted(self):
  d={"nativeViews":[view("UIHostingView"),view("UITabBar"),view("RCTViewComponentView","音量滑块")]};self.assertTrue(ready(d,"player"))
 def test_empty_window_is_rejected(self):self.assertFalse(ready({"nativeViews":[]},"player"))
if __name__=="__main__":unittest.main()
