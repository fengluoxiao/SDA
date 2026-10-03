"""Select an actually installed SDK; do not claim a skipped future SDK passed."""
import glob,os,re,subprocess,sys
sdk=sys.argv[1]
paths=glob.glob('/Applications/Xcode_'+sdk+'*.app')
def key(p):return tuple(map(int,re.findall(r'\d+',os.path.basename(p))))
chosen=None
for path in sorted(paths,key=key,reverse=True):
 env=dict(os.environ,DEVELOPER_DIR=path+'/Contents/Developer')
 info=subprocess.check_output(['xcodebuild','-showsdks'],env=env,text=True)
 if '-sdk iphoneos'+sdk+'.' in info:chosen=path;break
with open(os.environ['GITHUB_OUTPUT'],'a') as f:f.write('available='+str(chosen is not None).lower()+'\n')
if chosen:
 with open(os.environ['GITHUB_ENV'],'a') as f:f.write('DEVELOPER_DIR='+chosen+'/Contents/Developer\n')
else:
 text='iOS '+sdk+' SDK unavailable on this runner; compatibility NOT validated.\n'
 open('ios-artifacts/sdk-unavailable.txt','w').write(text)
 with open(os.environ['GITHUB_STEP_SUMMARY'],'a') as f:f.write(text)
 if sdk=='26':raise SystemExit(text)
