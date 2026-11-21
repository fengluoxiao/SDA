"""Reproducible in-process adaptation of pinned Mesh2HRTF NumCalc.
No Python is needed on device. This build-time generator leaves BEM maths intact.
"""
from pathlib import Path
import re,hashlib,json
ROOT=Path(__file__).resolve().parents[1]
SOURCE=ROOT/'vendor/mesh2hrtf-numcalc'
OUT=ROOT/'apps/mobile/modules/sda-core/ios/PersonalHrtfNative/Generated'
OUT.mkdir(exist_ok=True)
manifest=json.loads((SOURCE/'UPSTREAM.json').read_text())
for name,digest in manifest['files'].items():
    assert hashlib.sha256((SOURCE/name).read_bytes()).hexdigest()==digest,name

def clean(text):
    text=re.sub(r'/\*.*?\*/|//[^\n]*','',text,flags=re.S)
    text=re.sub(r'#ifdef USE_LAPACK.*?#endif','',text,flags=re.S)
    text=re.sub(r'^\s*#include[^\n]*','',text,flags=re.M)
    return text

def globals_in(text):
    depth=0;start=0;found=[]
    for i,c in enumerate(text):
        if c=='{':depth+=1
        elif c=='}':
            depth-=1
            if depth==0:start=i+1
        elif c==';' and depth==0:
            stmt=text[start:i+1].strip();start=i+1
            if not stmt or stmt.startswith(('extern','using','typedef','#')):continue
            if '(' in stmt and not re.search(r'\bsqrt|^(?:Vector|Matrix)<',stmt):continue
            if re.match(r'(?:static\s+)?(?:const\s+)?(?:int|bool|double|char|FILE|Complex|string|ofstream|ElCluster|IpCluster|ClusterLev|D_mtx_lev|S_mtx_lev|T_mtx_lev|(?:Vector|Matrix)<\w+>)(?=\s)',stmt):found.append(stmt)
    return found

files={p.name:clean(p.read_text()) for p in sorted(SOURCE.glob('*.cpp'),key=lambda p:p.name)}
fields=[];names=[]
for name,text in files.items():
    for stmt in globals_in(text):
        if stmt.startswith('const '):continue
        text=text.replace(stmt,'',1)
        if stmt=='ofstream NCout;':continue # the invocation owns its log
        fields.append(stmt.removeprefix('static '))
    text=text.replace('static int imultipori, nlevmlfmori;','')
    files[name]=text
fields.append('int imultipori, nlevmlfmori;')
fields.append('int *nearColumnBias;')
for field in fields:
    declaration=re.sub(r'^(?:(?:Vector|Matrix)<\w+>|\w+)\s+','',field).rstrip(';')
    # Commas within sqrt's argument do not occur in upstream declarations.
    parts=re.split(r',(?=[^()]*?(?:\(|$))',declaration) if '(' in declaration else declaration.split(',')
    for declarator in parts:
        names.append(re.match(r'[\s*]*([A-Za-z_]\w*)',declarator).group(1))
assert len(names)==len(set(names)), 'duplicate state names'

def allocations(text):
    pattern=re.compile(r'\bnew\s+([A-Za-z_]\w*\s*\*?)\s*\[')
    while True:
        m=pattern.search(text)
        if not m:break
        start=m.end();i=start;depth=1
        while depth:
            if text[i]=='[':depth+=1
            elif text[i]==']':depth-=1
            i+=1
        text=text[:m.start()]+'allocate_array<'+m[1].strip()+'>(('+text[start:i-1]+'))'+text[i:]
    text=re.sub(r'\bdelete\s*\[\s*\]\s*([^;]+);',r'release_array(\1);',text)
    text=re.sub(r'\bexit\s*\(', 'fail_exit(',text)
    assert not re.search(r'\bnew\b|\bdelete\b|\bexit\s*\(',text)
    return text

headers={name:clean((SOURCE/name).read_text()) for name in ['NC_Macros.h','NC_TypeDefinition.h','NC_ConstantsVariables.h','NC_Arrays.h','NC_IntegrationConstants.h']}
for name in ['NC_ConstantsVariables.h','NC_Arrays.h']:
    headers[name]=re.sub(r'\bextern\s+[^;]+;',lambda m:m[0] if '(' in m[0] else '',headers[name])
headers['NC_ConstantsVariables.h']=re.sub(r'#if !\(.*?#endif','',headers['NC_ConstantsVariables.h'],flags=re.S)
headers['NC_TypeDefinition.h']=allocations(headers['NC_TypeDefinition.h'])
def emit(name,text):
    (OUT/name).write_bytes(text.encode('utf-8'))
for name,text in headers.items():emit(name,text)
state='\n'.join(fields)
constructors=list(re.finditer(r'\b('+ '|'.join(names) +r')\(',state))
for match in reversed(constructors):
    i=match.end();depth=1
    while depth:
        if state[i]=='(':depth+=1
        elif state[i]==')':depth-=1
        i+=1
    state=state[:match.end()-1]+'{'+state[match.end():i-1]+'}'+state[i:]

state=state.replace('GauFMBOrd3*GauFMBOrd3','4').replace('GaupFMBOrde*GaupFMBOrde','4')
emit('State.inc','struct State {\n'+state+'\n};\nthread_local State *activeState=nullptr;\nState &state(){return *activeState;}\n')
def rename_function(text,function,mapping):
    for m in list(re.finditer(r'\b'+function+r'\s*\(',text)):
        i=m.end();depth=1
        while depth:
            if text[i]=='(':depth+=1
            elif text[i]==')':depth-=1
            i+=1
        j=i
        while j<len(text) and text[j].isspace():j+=1
        if j==len(text) or text[j]!='{':continue
        end=j+1;depth=1
        while depth:
            if text[end]=='{':depth+=1
            elif text[end]=='}':depth-=1
            end+=1
        chunk=text[m.start():end]
        for old,new in mapping.items():chunk=re.sub(r'\b'+old+r'\b',new,chunk)
        return text[:m.start()]+chunk+text[end:]
    raise AssertionError(function)
def replace_body(text,function,body):
    match=re.search(r'\b'+function+r'\s*\([^;{}]*\)\s*\{',text)
    assert match,function
    begin=match.end();end=begin;depth=1
    while depth:
        if text[end]=='{':depth+=1
        elif text[end]=='}':depth-=1
        end+=1
    return text[:begin]+'\n'+body+'\n'+text[end-1:]
# Upstream assumes a Windows text stream strips CR from CRLF. Parse bounded
# whitespace-separated tokens explicitly for iOS/POSIX, including tabs and EOF.
files['NC_Input.cpp']=replace_body(files['NC_Input.cpp'],'NC_GetLine',r"""
 if(!fgets(chinpline,SIZE,inputFile_))throw Failure(1,"Unexpected end of acoustic input");
 std::string line(chinpline);
 if(line.size()>=SIZE-1 && line.back()!='\n')throw Failure(1,"Acoustic input line exceeds bound");
 for(int i=0;i<NTRM;++i)chterms[i].clear();
 std::istringstream tokens(line);tokens.imbue(std::locale::classic());
 std::string token;int count=0;
 while(tokens>>token){
  if(token.find('#')!=std::string::npos)break;
  if(count==NTRM)throw Failure(1,"Too many acoustic input fields");
  chterms[count++]=token;
 }
 return count;
""")
files['NC_Input.cpp']=replace_body(files['NC_Input.cpp'],'NC_String2Integer',r"""
 char *end=nullptr;errno=0;const char *text=Ori_Str.c_str();
 long value=std::strtol(text,&end,10);
 if(errno==ERANGE || end==text || *end!='\0' || value<std::numeric_limits<int>::min() || value>std::numeric_limits<int>::max())throw Failure(1,"Invalid bounded acoustic integer");
 return int(value);
""")
files['NC_Input.cpp']=replace_body(files['NC_Input.cpp'],'NC_String2Double',r"""
 char *end=nullptr;errno=0;const char *text=Ori_Str.c_str();
 double value=std::strtod(text,&end);
 if(errno==ERANGE || end==text || *end!='\0' || !std::isfinite(value))throw Failure(1,"Invalid finite acoustic number");
 return value;
""")
# Bound dimension products before upstream signed-int FMM/BEM indexing. A
# numerical budget alone cannot catch an already-wrapped rows*rows product.
# Oversized scans need verified on-device mesh preparation, never unchecked
# allocation or an automatic low-detail ear replacement.
node_marker='numNodes_ = NC_String2Integer(chterms[2]);'
assert node_marker in files['NC_Input.cpp']
files['NC_Input.cpp']=files['NC_Input.cpp'].replace(node_marker,node_marker+"\n if(numElements_>20000 || numNodes_>20000 || numElementGroups_>128)throw Failure(3,\"Acoustic mesh dimensions exceed bounded native indexing\");",1)
# Reporting multiple matrix counts must not silently wrap a 32-bit estimate.
files['NC_Main.cpp']=files['NC_Main.cpp'].replace('uint nint = 0;','uint64_t nint = 0;').replace('uint ncmplx = 0;','uint64_t ncmplx = 0;')
# Compress address tables only. Keep Memory.txt as upstream's conservative
# (expanded-table) matrix estimate, not a promise about total process RSS.
def edit(file, old, new, count=1):
    if files[file].count(old)!=count: raise ValueError(f'Unexpected NumCalc anchor in {file}: {old}')
    files[file]=files[file].replace(old,new)

addr='NC_Addresses.cpp'; common='NC_CommonFunctions.cpp'; equation='NC_EquationSystem.cpp'
edit('NC_Main.cpp','    if( estimate_ram ) {','    if(!estimate_ram || runtime->capacityOnly)check_coefficient_storage();\n    if(runtime->capacityOnly)continue;\n    if( estimate_ram ) {')
# Keep edge validation on the tracked heap rather than a non-standard VLA.
edit('NC_Input.cpp','int hedges[numNodes_];','Vector<int> hedges(numNodes_);')
files[addr]=replace_body(files[addr],'NC_AllocateNmtx',clean((ROOT/'scripts/numcalc-compact-near.inc').read_text()))
edit(addr,'delete [] jcolnea;', 'delete [] nearColumnBias; delete [] jcolnea;')
edit(common,'jcolnea[j]', 'jcolnea[j+nearColumnBias[i]]',6)
edit(common,'jcolnea[k] == nL_colnu[j]', 'jcolnea[k+nearColumnBias[i]] == nL_colnu[j]')
edit(common,'jcolnea[k] == i', 'jcolnea[k+nearColumnBias[nU_rownu[j]]] == i')
edit(equation,'jcolnea[i] == ncol', 'jcolnea[i+nearColumnBias[nrow]] == ncol')

# Single-level FMM. T repeats its cluster's DOFs for every integration
# direction; D shifts a cluster's existing far-neighbor IDs by the direction;
# S is a contiguous interval per row. None requires a full column table.
edit(addr,'new int[nnonzertmtx]', 'new int[nnonzertmtx/numIntegrationPointsUnitSphere_]')
edit(addr,'for(k=0; k<clustarry[i].NumOfEl; k++)\n            {\n                k1',
     'if (j != 0) { nnoze += clustarry[i].NumOfDOFs; continue; }\n            int compactT = nnoze/numIntegrationPointsUnitSphere_;\n            for(k=0; k<clustarry[i].NumOfEl; k++)\n            {\n                k1')
edit(addr,'jcoltmtx[nnoze++] = jelist[k1][0];', 'jcoltmtx[compactT++] = jelist[k1][0]; ++nnoze;')
edit(addr,'if(idfs == 2) jcoltmtx[nnoze++] = jelist[k1][1];','if(idfs == 2) { jcoltmtx[compactT++] = jelist[k1][1]; ++nnoze; }')
edit(common,'jcoltmtx[j]', 'jcoltmtx[(j-irowtmtx[i])+irowtmtx[i-i%numIntegrationPointsUnitSphere_]/numIntegrationPointsUnitSphere_]')
edit(addr,'dmtxlev[0].jcolDmxLv = new int[nnonzerdmtx];','dmtxlev[0].jcolDmxLv = nullptr;')
edit(addr,'for(j=0; j<clustarry[i].NumFarClus; j++)\n                dmtxlev[0].jcolDmxLv[nnze++] = clustarry[i].NumsFarClus[j]*numIntegrationPointsUnitSphere_ + k;', 'nnze += clustarry[i].NumFarClus;')
edit(addr,'jcolsmtx = new int[nnonzersmtx];','jcolsmtx = new int[numRowsOfCoefficientMatrix_];')
edit(addr,'for(j=0; j<numIntegrationPointsUnitSphere_; j++) jcolsmtx[itr++] = kcol++;','jcolsmtx[i] = kcol-itr; itr += numIntegrationPointsUnitSphere_;')
for unit in (common,equation):
    edit(unit,'dmtxlev[0].jcolDmxLv[j]', '(clustarry[i/numIntegrationPointsUnitSphere_].NumsFarClus[j-dmtxlev[0].irowDmxLv[i]]*numIntegrationPointsUnitSphere_+i%numIntegrationPointsUnitSphere_)')
    edit(unit,'jcolsmtx[j]', '(j+jcolsmtx[i])')
# Aggregate budgets can now exceed 1 GiB, but CSR offsets remain signed ints.
# Check the actual matrix counts before multiplying/allocating upstream arrays.
for var in ('nnonzerdmtx','nnonzertmtx'):
    edit(addr,var+' *= numIntegrationPointsUnitSphere_;',var+' = index_product('+var+',numIntegrationPointsUnitSphere_);')
edit(addr,'nnonzerdmtx += clustarry[i].NumFarClus;', 'nnonzerdmtx = index_sum(nnonzerdmtx,clustarry[i].NumFarClus);')
edit(addr,'nnonzerdmtx += nclufar[i];', 'nnonzerdmtx = index_sum(nnonzerdmtx,nclufar[i]);')
edit(addr,'nnonzerdmtx *= clulevarry[nlv].nPoinSpheLv;', 'nnonzerdmtx = index_product(nnonzerdmtx,clulevarry[nlv].nPoinSpheLv);')
edit(addr,'nEntriesT = j*clulevarry[nlv].nPoinSpheLv;', 'nEntriesT = index_product(j,clulevarry[nlv].nPoinSpheLv);')
edit(addr,'Matrix<int> nuclfar(numOriginalClusters_, numOriginalReflectedClusters_);', 'index_product(numOriginalClusters_,numOriginalReflectedClusters_);\n    Matrix<int> nuclfar(numOriginalClusters_, numOriginalReflectedClusters_);')
# Bounded ML-FMM retains the upstream near matrix and cluster tree. Never
# allocate expanded column/complex arrays just to compress them afterwards.
edit(addr,'tmtxlev[nlv].jcolTmxLv = new', 'if(runtime->compactFar){tmtxlev[nlv].jcolTmxLv=nullptr;tmtxlev[nlv].irowTmxLv=nullptr;continue;}\n\t\ttmtxlev[nlv].jcolTmxLv = new')
edit(addr,'smtxlev[nlv].jcolSmxLv = new', 'if(runtime->compactFar){smtxlev[nlv].jcolSmxLv=nullptr;smtxlev[nlv].irowSmxLv=nullptr;continue;}\n\t\tsmtxlev[nlv].jcolSmxLv = new')
edit(addr,'dmtxlev[nlv].jcolDmxLv = new','if(runtime->compactFar){dmtxlev[nlv].jcolDmxLv=nullptr;dmtxlev[nlv].irowDmxLv=nullptr;dmtxlev[nlv].nRowsD=nrosdmtx;dmtxlev[nlv].nEntriesD=nnonzerdmtx;continue;}\n        dmtxlev[nlv].jcolDmxLv = new')
for m,t in [('t','T'),('d','D'),('s','S')]:
    field=f'{m}mtxlev[j].z{t}mxLv = '
    edit('NC_Main.cpp',field+'new Complex',field+'runtime->compactFar ? nullptr : new Complex')
edit('NC_Main.cpp','NC_PostProcessing(NCout);','packed_release(); NC_PostProcessing(NCout);')
edit('NC_Main.cpp','delete [] clulevarry[j].zwkS;', 'delete [] clulevarry[j].zwkS;\n        for(int c=0;c<clulevarry[j].nClustOLv;++c){delete [] clulevarry[j].ClastArLv[c].NumsFanClus; clulevarry[j].ClastArLv[c].NumsFanClus=nullptr;}')
edit(equation,'            NC_BuildDforMLFMM(NCout);','            if(runtime->compactFar)packed_prepare();\n            NC_BuildDforMLFMM(NCout);')
edit(equation,'            NC_BuildSforMLFMM(NCout);','            if(!runtime->compactFar)NC_BuildSforMLFMM(NCout);')
edit(equation,'for(i=0; i<irowTmxLn[nrowT]; i++)','if(!runtime->compactFar)for(i=0; i<irowTmxLn[nrowT]; i++)')
edit(equation,'zTmxLn[iadrek] = TMele[k]*dfa_cl;', 'if(!runtime->compactFar)zTmxLn[iadrek] = TMele[k]*dfa_cl;')
edit(equation,'jdifdar = j*ndofi;\n                if(rfa_cl', 'if(runtime->compactFar)packed_store_t(nlv,i,j,TMele);\n                jdifdar = j*ndofi;\n                if(rfa_cl')
edit(equation,'if(irowTmxLn[nrowT] != iadrek + 1)', 'if(!runtime->compactFar && irowTmxLn[nrowT] != iadrek + 1)')
edit(equation,'jcl = clulevarry[nlv].ClastArLv[i].NumsFanClus[j];','jcl = clulevarry[nlv].ClastArLv[i].NumsFanClus[j];\n            if(runtime->compactFar && jcl<i)continue;')
edit(equation,'if( adapt_fmmlength_) {','if(adapt_fmmlength_ || runtime->compactFar) {')
edit(equation,'NC_SphericalHankel(NCout, zH1m, expalength,dkij);','if(runtime->compactFar)expalength=std::min(clulevarry[nlv].nExpaTermLv,std::max(minExpansionTermsFMM_,int(std::floor(rw+.5))+5));\n              NC_SphericalHankel(NCout, zH1m, expalength,dkij);')
edit(equation,'ikjadre = identry + k*clulevarry[nlv].ClastArLv[i].NumFanClus + j;', 'if(runtime->compactFar){packed_store_d(nlv,i,j,zdiat);break;}\n                    ikjadre = identry + k*clulevarry[nlv].ClastArLv[i].NumFanClus + j;')
edit(equation,'Vector<Complex> zvc_d(maxRowNumberD_);','if(runtime->compactFar){packed_apply(nullptr,nullptr);return;}\n\tVector<Complex> zvc_d(maxRowNumberD_);')
edit(common,'for(i=0; i<clulevarry[ilv].nPoinSpheLv*clulevarry[ilv].nClustSLv; i++)','if(runtime->compactFar){packed_apply(&ztmp,&zresuvect);break;}\n\t\t\tfor(i=0; i<clulevarry[ilv].nPoinSpheLv*clulevarry[ilv].nClustSLv; i++)')
edit(common,'for(j=smtxlev[ilv].irowSmxLv[i]; j<smtxlev[ilv].irowSmxLv[i + 1]; j++)\n\t\t\t\t\tsmtxlev[ilv].zSmxLv[j] *= dwk;', 'if(runtime->compactFar){if(ilv==0)runtime->packedRowScale[i]*=dwk;}\n                else for(j=smtxlev[ilv].irowSmxLv[i]; j<smtxlev[ilv].irowSmxLv[i + 1]; j++)smtxlev[ilv].zSmxLv[j] *= dwk;')
edit(common,'if(err_rel < ErroIterSols || j == niter_max_)','if(runtime->compactFar && (!std::isfinite(err_rel) || (j==niter_max_ && err_rel>=ErroIterSols)))throw Failure(1,"Bounded ML-FMM did not converge");\n        if(err_rel < ErroIterSols || j == niter_max_)')
# Rename parameters that shadow former globals before qualifying global uses.
# Keep C++ lexical shadowing intact (including NC_GetLine's temporary files).
for filename,text in files.items():
    definitions=[]
    for match in re.finditer(r'\bNC_\w+\s*\(',text):
        i=match.end();depth=1
        while depth:
            if text[i]=='(':depth+=1
            elif text[i]==')':depth-=1
            i+=1
        j=i
        while j<len(text) and text[j].isspace():j+=1
        if j==len(text) or text[j]!='{':continue
        end=j+1;depth=1
        while depth:
            if text[end]=='{':depth+=1
            elif text[end]=='}':depth-=1
            end+=1
        params=text[match.end():i-1]
        shadowed=re.findall(r'(?:\b(?:FILE|double|int|Complex)|(?:Vector|Matrix)<\w+>)\s*[*&\s]+('+'|'.join(names)+r')\b',params)
        if shadowed:definitions.append((match.start(),end,shadowed))
    for begin,end,shadowed in reversed(definitions):
        chunk=text[begin:end]
        for name in set(shadowed):chunk=re.sub(r'(?<![.>])\b'+name+r'\b','arg_'+name,chunk)
        text=text[:begin]+chunk+text[end:]
    # Argument names in forward declarations are immaterial; remove only names
    # after a pointer/reference type, leaving their declared types unchanged.
    text=re.sub(r'(^\s*(?:extern\s+)?(?:void|int|double|bool|Complex)\s+NC_\w+\([^;{}]*\);)',lambda m: re.sub(r'(?<=[*&])\s*('+'|'.join(names)+r')\b','',m[0]),text,flags=re.M)
    files[filename]=text
for function in ['NC_ComputeEntriesForTBEM','NC_ComputeEntriesNforFMM']:
    files['NC_EquationSystem.cpp']=rename_function(files['NC_EquationSystem.cpp'],function,{'Admib3':'localAdmittance'})
# These helpers use the upstream global rather than accepting the log by
# reference. Share the invocation's real stream with the rest of the solver.
for unit,functions in [('NC_3dFunctions.cpp',['NC_ComputeGlobalCoordinates','NC_ComputeUnitVectorOnElement','NC_ReflectPointVector','NC_ReflectElement','NC_ComputeGausseanPoints']),
                       (common,['BLdispoi','NC_MatrixVectorMultiplication'])]:
    for function in functions:files[unit]=rename_function(files[unit],function,{'NCout':'runtime->jobLog'})
for name in files:
    files[name]=re.sub(r'extern\s+(?:bool|int|double)\s+\w+\s*;','',files[name])
word=re.compile(r'(?<![.>])\b('+'|'.join(sorted(names,key=len,reverse=True))+r')\b')
for name,text in files.items():
    text=allocations(text)
    text=text.replace('FILE* inputFile_,','FILE*,')
    text=word.sub(lambda m:'state().'+m[0],text)
    text=re.sub(r'\bfopen\s*\(','local_open(',text)
    text=re.sub(r'\bfclose\s*\(','local_close(',text)
    text=re.sub(r'\b_?mkdir\s*\(','local_mkdir(',text)
    text=re.sub(r'local_mkdir\(([^,;\n]+),[^);\n]+\)',r'local_mkdir(\1)',text)
    text=re.sub(r'\bsystem\([^)]*\)','0',text)
    text=text.replace('ofstream NCout(filename);','auto &NCout=runtime->jobLog; NCout.open(log_path(filename));')
    text=text.replace('NCestim.open("Memory.txt");','NCestim.open(log_path("Memory.txt"));')
    text=re.sub(r'gethostname\([^;]*;', '',text)
    text=re.sub(r'\b(cout|cerr)\b','runtime->quiet',text)
    text=text.replace('localtime(&lot)','local_walltime(lot)')
    text=re.sub(r'fprintf\(stdout,[^;]+;', '',text)
    if name=='NC_Main.cpp':
        text=text.replace('int main(int argc, char **argv)','int embedded_main(int argc, char **argv)')
        marker='for(state().currentFrequency_ = state().istart_; state().currentFrequency_ < iend; state().currentFrequency_++) {'
        assert marker in text
        text=text.replace(marker,marker+'\nfrequency_progress(state().currentFrequency_, state().numFrequencies_);')
    # Cooperative cancellation at loops with a compound body. Destructors and
    # cleanup helpers in the template headers intentionally have no checkpoints.
    positions=[]
    for m in re.finditer(r'\b(?:for|while)\s*\(',text):
        i=m.end();depth=1
        while depth:
            if text[i]=='(':depth+=1
            elif text[i]==')':depth-=1
            i+=1
        j=i
        while j<len(text) and text[j].isspace():j+=1
        if j<len(text) and text[j]=='{':positions.append(j+1)
    for i in reversed(positions):text=text[:i]+' checkpoint(); '+text[i:]
    emit(name+'.inc',text)
emit('provenance.json',json.dumps({'upstreamCommit':manifest['commit'],'licence':'EUPL-1.2','generatedBy':'scripts/generate-embedded-numcalc.py','jobStateFields':len(names),'changes':['private per-invocation state','tracked native arrays','shared near-field and compact single-level FMM indices','bounded reciprocal ML-FMM for rigid triangles','in-process errors and cancellation','job-relative file IO','no subprocess or cwd changes']},indent=2)+'\n')
print('Generated embedded NumCalc with',len(names),'per-invocation state fields')
