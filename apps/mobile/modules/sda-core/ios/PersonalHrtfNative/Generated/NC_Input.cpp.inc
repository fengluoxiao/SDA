








using namespace std;                                                                





void NC_ReadBasicParametersB(ofstream&,FILE*,char*,string[],double*);
void NC_ReadMesh(ofstream&,FILE*,char*, string[], bool);
void NC_ReadBoundaryConditions(ofstream&,FILE*,char*,string[],int&,int&,inputLineBoundaryCondition *);
void NC_ReadSoundSources(ofstream&,FILE*,char*, string[]);
void NC_ReadFrequencyCurves(ofstream&,FILE*,char*, string[]);
void NC_StoreBoundaryConditions(ofstream&,FILE*,char*,string[],int&,inputLineBoundaryCondition *);
void NC_WriteParameters(ofstream&,double*);
void NC_ComputeFrequencies(ofstream&,int&, double*,double*,double*);
int NC_GetLine(FILE *,char *,string[]);
int NC_String2Integer(string);
double NC_String2Double(string Ori_Str);
bool NC_checkedges(void);



void NC_Read
(
	ofstream& NCout,
	FILE* arg_inputFile_,			
	char* chinpline,		
	string chterms[NTRM],	
	double* Freqs,			
	bool check_normals
)
{
	int i, nabspsourc, maxbcline3, nbcline3;
	inputLineBoundaryCondition *bouconlin3;

	
	NC_ReadBasicParametersB(NCout, arg_inputFile_, chinpline, chterms, Freqs);

	
	
	state().extNumbersOfNodes = allocate_array<int>((state().numNodes_));
	state().isNodeMeshOrEvalNode = allocate_array<int>((state().numNodes_));
	state().extNumbersOfElements = allocate_array<int>((state().numElements_));

	state().nodesCoordinates = allocate_array<double*>((state().numNodes_));
	for(i=0; i<state().numNodes_; i++) state().nodesCoordinates[i] = allocate_array<double>((NDIM));

	state().elementsConnectivity = allocate_array<int*>((state().numElements_));
	for(i=0; i<state().numElements_; i++) state().elementsConnectivity[i] = allocate_array<int>((NNODPE));

	state().listNumberNodesPerElement = allocate_array<int>((state().numElements_));
	state().listElementProperty = allocate_array<int>((state().numElements_));
	state().listElementsElementGroup = allocate_array<int>((state().numElements_));

	state().listSymmetryPlanes = allocate_array<int>((NDIM));
	state().listSymmetryPlanesCoordinates = allocate_array<double>((NDIM));

	state().ibval = allocate_array<int>((state().numElements_));
	state().zbval0 = allocate_array<Complex*>((state().numElements_));
	for(i=0; i<state().numElements_; i++) state().zbval0[i] = allocate_array<Complex>((NNODPE));
	state().zbvao0 = allocate_array<Complex*>((state().numElements_));
	for(i=0; i<state().numElements_; i++) state().zbvao0[i] = allocate_array<Complex>((NNODPE));
	state().zbval1 = allocate_array<Complex>((state().numElements_));
	state().zbval2 = allocate_array<Complex>((state().numElements_));
	state().zbvao1 = allocate_array<Complex>((state().numElements_));
	state().zbvao2 = allocate_array<Complex>((state().numElements_));

	state().elenor = allocate_array<double*>((state().numElements_));
	for(i=0; i<state().numElements_; i++) state().elenor[i] = allocate_array<double>(((NNODPE + 1)*NDIM));
	state().centel = allocate_array<double*>((state().numElements_));
	for(i=0; i<state().numElements_; i++) state().centel[i] = allocate_array<double>((NDIM));
	state().areael = allocate_array<double>((state().numElements_));
	state().jelist = allocate_array<int*>((state().numElements_));
	for(i=0; i<state().numElements_; i++) state().jelist[i] = allocate_array<int>((2));
	for(i=0; i<state().numElements_; i++) state().jelist[i][0] = state().jelist[i][1] = -1;

	state().indexOfElementGroup = allocate_array<int>((state().numElementGroups_));
	state().numberOfElementsInGroup = allocate_array<int>((state().numElementGroups_));
	state().propertyOfGroup = allocate_array<int>((state().numElementGroups_));

	state().numinw = allocate_array<int>((state().numIncidentPlaneWaves_));
	nabspsourc = abs(state().numPointSources_);
	state().numposo = allocate_array<int>((nabspsourc));
	state().dirinw = allocate_array<double*>((state().numIncidentPlaneWaves_));
	for(i=0; i<state().numIncidentPlaneWaves_; i++) state().dirinw[i] = allocate_array<double>((NDIM));
	state().coorps = allocate_array<double*>((nabspsourc));
	for(i=0; i<nabspsourc; i++) state().coorps[i] = allocate_array<double>((NDIM));
	state().inwacurv = allocate_array<int*>((state().numIncidentPlaneWaves_));
	for(i=0; i<state().numIncidentPlaneWaves_; i++) state().inwacurv[i] = allocate_array<int>((2));
	state().posocurv = allocate_array<int*>((nabspsourc));
	for(i=0; i<nabspsourc; i++) state().posocurv[i] = allocate_array<int>((2));
	state().zinwav = allocate_array<Complex>((state().numIncidentPlaneWaves_));
	state().zposoc = allocate_array<Complex>((nabspsourc));
	state().zinwao = allocate_array<Complex>((state().numIncidentPlaneWaves_));
	state().zposoo = allocate_array<Complex>((nabspsourc));

	
	
	
	
	NC_ReadMesh(NCout, arg_inputFile_, chinpline, chterms, check_normals);


	
	
	
	maxbcline3 = 2*state().numElementsOfBoundaryMesh_;
	bouconlin3 = allocate_array<inputLineBoundaryCondition>((maxbcline3));
	NC_ReadBoundaryConditions(NCout, arg_inputFile_, chinpline, chterms, maxbcline3,
		nbcline3, bouconlin3);

	
	if(state().numIncidentPlaneWaves_ > 0 || state().numPointSources_ > 0 || state().numPointSources_ == -1)
		NC_ReadSoundSources(NCout, arg_inputFile_, chinpline, chterms);

	
	NC_ReadFrequencyCurves(NCout, arg_inputFile_, chinpline, chterms);

	
	NC_StoreBoundaryConditions(NCout, arg_inputFile_, chinpline, chterms, nbcline3,
		bouconlin3);
	release_array(bouconlin3);

	
	NC_WriteParameters(NCout, Freqs);
}


void NC_ReadBasicParametersA
(
	ofstream& NCout,
	FILE* arg_inputFile_,			
	char* chinpline,		
	string chterms[NTRM]	
)
{
    int i, nterms, j, j1;

    
    nterms = 0;
    while(!nterms){ checkpoint(); nterms = NC_GetLine(arg_inputFile_, chinpline, chterms);}
    if(chterms[0].compare("Mesh2HRTF")) NC_Error_Exit_0(NCout, "Key word Mesh2HRTF expected!");

    j1 = 0;
    
    state().versionNumber_ = chterms[0];
    
    for(i=1; i<nterms; i++)
    { checkpoint(); 
      
      state().versionNumber_ = state().versionNumber_ + " " + chterms[i];
    }

    
    nterms = 0;
    
    while(!nterms){ checkpoint(); nterms = NC_GetLine(arg_inputFile_, chinpline, chterms);}
    j1 = 0;
    
    state().jobTitle_ = chterms[0];
    for(i=1; i<nterms; i++)
    { checkpoint(); 
      state().jobTitle_ = state().jobTitle_ + " " + chterms[i];
      
    }

    runtime->quiet << "Job title: " << state().jobTitle_ << endl;

    
    while(!NC_GetLine(arg_inputFile_, chinpline, chterms));

    
    while(!NC_GetLine(arg_inputFile_, chinpline, chterms));
    state().numFrequencies_ = NC_String2Integer(chterms[1]);
    state().frequencyStepIncrement_ = NC_String2Double(chterms[2]);
    state().firstFrequencyStep_ = NC_String2Double(chterms[3]);
}


void NC_ReadBasicParametersB
(
	ofstream& NCout,
	FILE* arg_inputFile_,			
	char* chinpline,		
	string chterms[NTRM],	
	double* Freqs			
)
{

	int i, npoicurv;
	double *p_dcurvfrq, *p_dcurvtime;

	while(!NC_GetLine(arg_inputFile_, chinpline, chterms));
	npoicurv = NC_String2Integer(chterms[1]);

	p_dcurvfrq = allocate_array<double>((npoicurv));
	p_dcurvtime = allocate_array<double>((npoicurv));

	for(i=0; i<npoicurv; i++)
	{ checkpoint(); 
		while(!NC_GetLine(arg_inputFile_, chinpline, chterms));
		p_dcurvtime[i] = NC_String2Double(chterms[0]);
		p_dcurvfrq[i] = NC_String2Double(chterms[1]);
	}

	
	NC_ComputeFrequencies(NCout, npoicurv, p_dcurvfrq, p_dcurvtime, Freqs);

	release_array(p_dcurvfrq);
	release_array(p_dcurvtime);

	
	while(!NC_GetLine(arg_inputFile_, chinpline, chterms));
	state().numElementGroups_ = NC_String2Integer(chterms[0]);
	state().numElements_ = NC_String2Integer(chterms[1]);
	state().numNodes_ = NC_String2Integer(chterms[2]);
 if(state().numElements_>20000 || state().numNodes_>20000 || state().numElementGroups_>128)throw Failure(3,"Acoustic mesh dimensions exceed bounded native indexing");
	state().minExpansionTermsFMM_ = NC_String2Integer(chterms[3]);
	state().numSymmetricPlanes_ = NC_String2Integer(chterms[4]);
	state().methodBEM_ = NC_String2Integer(chterms[7]);
	state().methodSolver_ = NC_String2Integer(chterms[8]);

	if(state().numElements_ < 1) NC_Error_Exit_1(NCout, "Number of elements must > 0!",
		"Number of elements = ", state().numElements_);

	if(state().numNodes_ < 1) NC_Error_Exit_1(NCout, "Number of nodes must > 0!", "Number of nodes = ", state().numNodes_);

	if(state().minExpansionTermsFMM_ == 0) state().minExpansionTermsFMM_ = 8;
	if(state().minExpansionTermsFMM_ < 5 || state().minExpansionTermsFMM_ > 10) {
		NC_Error_Exit_1(NCout, "state().minExpansionTermsFMM_  must be in [5 10]!", "state().minExpansionTermsFMM_ = ", state().minExpansionTermsFMM_);
    }

	if(state().numSymmetricPlanes_ < 0 || state().numSymmetricPlanes_ > 3)
		NC_Error_Exit_1(NCout, "Number of symmetry planes must be in [0 3]!",
		"Number of symmetry planes = ", state().numSymmetricPlanes_);

	if(state().methodSolver_ < 0 || state().methodSolver_ > 4)
        NC_Error_Exit_1(NCout, "ISOLVER must be 0 or 4!", "ISOLVER = ", state().methodSolver_);

    if(state().methodSolver_ > 0 && state().methodSolver_ < 4)
        NC_Error_Exit_1(NCout, "ISOLVER must be 0 or 4!", "ISOLVER = ", state().methodSolver_);

	switch(state().methodBEM_)    
	{
	case 0:
		state().methodFMM_ = 0; 
		break;
	case 1:
		state().methodFMM_ = 1; 
		break;
	case 4:
		state().methodFMM_ = 3; 
		break;
	default:
		NC_Error_Exit_1(NCout, "state().methodBEM_ must be in {0,1,4}!", "state().methodBEM_ = ", state().methodBEM_);
	}

	if(state().methodFMM_ && state().methodSolver_ == 4) state().methodSolver_ = 0;

	
	while(!NC_GetLine(arg_inputFile_, chinpline, chterms));
	state().numIncidentPlaneWaves_ = NC_String2Integer(chterms[0]);
	state().numPointSources_ = NC_String2Integer(chterms[1]);
	state().isInternalProblem_ = NC_String2Integer(chterms[2]);
	state().ClusEdgL0_ = NC_String2Double(chterms[3]);
	state().methodPreconditioner_ = NC_String2Integer(chterms[4]);

	if(state().numIncidentPlaneWaves_ < 0) NC_Error_Exit_1(NCout, "Number of the incident plane waves must >= 0!",
		"Number of the incident plane waves = ", state().numIncidentPlaneWaves_);
	if(state().numPointSources_ < -1) NC_Error_Exit_1(NCout, "Number of the point sources must >= -1!",
		"Number of the point sources = ", state().numIncidentPlaneWaves_);
	if(state().isInternalProblem_ != 0 && state().isInternalProblem_ != 1)
		NC_Error_Exit_1(NCout, "IFINTE must = 0 or 1!", "IFINTE = ", state().isInternalProblem_);
	if(state().methodPreconditioner_ < -3 || state().methodPreconditioner_ > 2)
		NC_Error_Exit_1(NCout, "IPRECOND must be in [-3 2]!", "IPRECOND = ", state().methodPreconditioner_);

	if(state().methodPreconditioner_ <= -3) {
		state().scanningDegreeLU_ = 3;
		state().methodPreconditioner_ = 0;
	} else if(state().methodPreconditioner_ == -2) {
		state().scanningDegreeLU_ = 2;
		state().methodPreconditioner_ = 0;
	} else if(state().methodPreconditioner_ == -1) {
		state().scanningDegreeLU_ = 1;
		state().methodPreconditioner_ = 0;
	} else {
		state().scanningDegreeLU_ = 0;
	}

	
	if(state().methodSolver_ == 4) state().methodPreconditioner_ = 2;

	
	while(!NC_GetLine(arg_inputFile_, chinpline, chterms));

	
	while(!NC_GetLine(arg_inputFile_, chinpline, chterms));
	state().speedOfSound_ = NC_String2Double(chterms[0]);
	state().densityOfMedium_ = NC_String2Double(chterms[1]);
	state().harmonicTimeFactor_ = NC_String2Double(chterms[2]);

	if(state().harmonicTimeFactor_ != -1.0) state().harmonicTimeFactor_ = 1.0;

    if(state().speedOfSound_ < 300.0) NC_Error_Warning_1(NCout, "state().speedOfSound_ perhaps too small!",
        "state().speedOfSound_ = ", state().speedOfSound_);
    if(state().densityOfMedium_ < 1.0) NC_Error_Warning_1(NCout, "state().densityOfMedium_ perhaps too small!",
        "state().densityOfMedium_ = ", state().densityOfMedium_);
    state().epsilon_ = 1.0e-6;

	
	state().numReflectionsOfElements_ = (int)pow(2.0, state().numSymmetricPlanes_);

	
	if(state().isInternalProblem_) state().Tao_ = -1.0; else state().Tao_ = 1.0;
}





void NC_ReadMesh
(
 ofstream& NCout,
 FILE* arg_inputFile_,			
 char* chinpline,		
 string chterms[NTRM],	
 bool check_normals  
 )
{
  int i, j = 0, k = 0, nterms;
  double areevl;
  
  
  while(!NC_GetLine(arg_inputFile_, chinpline, chterms));
  if(chterms[0].compare("NODES")) NC_Error_Exit_0(NCout, "Key word NODES expected!");

  
  i=0;
  while(i<state().numNodes_) { checkpoint(); 
    while(!NC_GetLine(arg_inputFile_, chinpline, chterms));
    state().tmpFile_ = local_open(chterms[0].c_str(), "r");
    if(state().tmpFile_) 
      {
	while(!NC_GetLine(state().tmpFile_, chinpline, chterms));
	int tmpFileLength = NC_String2Integer(chterms[0]);
	for(int jj=0; jj<tmpFileLength; jj++)
	  { checkpoint(); 
	    while(!NC_GetLine(state().tmpFile_, chinpline, chterms));
	    state().extNumbersOfNodes[i] = NC_String2Integer(chterms[0]);
	    state().nodesCoordinates[i][0] = NC_String2Double(chterms[1]);
	    state().nodesCoordinates[i][1] = NC_String2Double(chterms[2]);
	    state().nodesCoordinates[i][2] = NC_String2Double(chterms[3]);
	    i+=1;
	  }
      }
    else 
      {
	runtime->quiet << "No Nodes file found!" << endl;
	NCout << "No Nodes file found!" << endl;
	fail_exit(-1);
      }
    local_close(state().tmpFile_); 
  }

  
  while(!NC_GetLine(arg_inputFile_, chinpline, chterms));
  if(chterms[0].compare("ELEMENTS")) NC_Error_Exit_0(NCout, "Key word ELEMENTS expected!");
  
  i=0;
  while(i<state().numElements_) { checkpoint(); 
    while(!NC_GetLine(arg_inputFile_, chinpline, chterms));
    state().tmpFile_ = local_open(chterms[0].c_str(), "r");
    if(state().tmpFile_) 
      {
	while(!NC_GetLine(state().tmpFile_, chinpline, chterms));
	int tmpFileLength = NC_String2Integer(chterms[0]);
	
	for(int jj=0; jj<tmpFileLength; jj++)
	  { checkpoint(); 
	    nterms = 0;
	    while(!nterms){ checkpoint(); nterms = NC_GetLine(state().tmpFile_, chinpline, chterms);}
	    
	    if(nterms < 7 || nterms > 8) NC_Error_Exit_2(NCout,
							 "number of entris of a input line must be 7 or 8!",
							 "number of the element under the key word ELEMENTS = ", NC_String2Integer(chterms[0]),
							 "number of entrie of the line = ", nterms);
	    
	    state().extNumbersOfElements[i] = NC_String2Integer(chterms[0]);
	    state().listNumberNodesPerElement[i] = nterms - 4;
	    for(j=0; j<state().listNumberNodesPerElement[i]; j++) state().elementsConnectivity[i][j] = NC_String2Integer(chterms[j + 1]);
	    state().listElementProperty[i] = NC_String2Integer(chterms[nterms - 3]);
	    state().listElementsElementGroup[i] = NC_String2Integer(chterms[nterms - 1]);
	    i+=1;
	  }
      }
    else 
      {
	runtime->quiet << "No Elements file found!" << endl;
	NCout << "No Elements file found!" << endl;
	fail_exit(-1);
      }
    local_close(state().tmpFile_);
  }
  
  
  state().numElementsOfBoundaryMesh_ = state().numElementsOfEvaluationMesh_ = 0;
  for(i=0; i<state().numElements_; i++) { checkpoint(); 
    if(state().listElementProperty[i] == 0)
      state().numElementsOfBoundaryMesh_++;
    else if(state().listElementProperty[i] == 2) {
      if(state().numElementsOfEvaluationMesh_ == 0) {
	j = state().listElementsElementGroup[i];
	k = i;
      }
      else if(state().listElementsElementGroup[i] != j) {
	runtime->quiet << "\n" << "Element " << state().extNumbersOfElements[k] << " belongs to the el. group " << state().listElementsElementGroup[k] << " but" << endl;
	runtime->quiet << "Element " << state().extNumbersOfElements[i] << " belongs to the el. group " << state().listElementsElementGroup[i] << "!" << endl;
	NCout << "\n" << "Element " << state().extNumbersOfElements[k] << " belongs to the el. group " << state().listElementsElementGroup[k] << " but" << endl;
	NCout << "Element " << state().extNumbersOfElements[i] << " belongs to the el. group " << state().listElementsElementGroup[i] << "!" << endl;
	NC_Error_Exit_0(NCout, "Elements of the evaluation mesh must be of the same el. group!");
      }
      state().numElementsOfEvaluationMesh_++;
    }
    else NC_Error_Exit_2(NCout, "state().listElementProperty[i] must be 0 or 2!", "i = ", i, "state().listElementProperty[i] = ",  state().listElementProperty[i]);
    } 
  
  
  if(state().numSymmetricPlanes_ > 0) {
    while(!NC_GetLine(arg_inputFile_, chinpline, chterms));
    if(chterms[0].compare("SYMMETRY")) NC_Error_Exit_0(NCout, "Key word SYMMETRY expected!");
    
    while(!NC_GetLine(arg_inputFile_, chinpline, chterms));
    for(i=0; i<NDIM; i++) state().listSymmetryPlanes[i] = NC_String2Integer(chterms[i]);
    
    while(!NC_GetLine(arg_inputFile_, chinpline, chterms));
    for(i=0; i<NDIM; i++) state().listSymmetryPlanesCoordinates[i] = NC_String2Double(chterms[i]);
    
    j = 0;
    for(i=0; i<NDIM; i++)
      { checkpoint(); 
	if(state().listSymmetryPlanes[i]) j++;
	switch(state().listSymmetryPlanes[i])
	  {
	  case 0:
	  case 1:
	    break;
	  case 2:
	    state().listSymmetryPlanes[i] = -1;
	    break;
	  case 3:
	    state().listSymmetryPlanes[i] = 2;
	    break;
	  case 4:
	    state().listSymmetryPlanes[i] = -2;
	    break;
	  default:
	    NC_Error_Exit_2(NCout, "state().listSymmetryPlanes[i] must be equal to 0, 1, 2, 3, or 4!",
			    "i = ", i, "state().listSymmetryPlanes[i] = ", state().listSymmetryPlanes[i]);
	  }
      }
    if(j != state().numSymmetricPlanes_) NC_Error_Exit_2(NCout, "j must equal to state().numSymmetricPlanes_", "j = ", j,
						 "state().numSymmetricPlanes_ = ", state().numSymmetricPlanes_);
  }
  
  
  for(i=0; i<state().numElements_; i++) { checkpoint(); 
    for(j=0; j<state().listNumberNodesPerElement[i]; j++) { checkpoint(); 
      for(k=0; k<state().numNodes_; k++) { checkpoint(); 
	if(state().elementsConnectivity[i][j] == state().extNumbersOfNodes[k])
	  {
	    state().elementsConnectivity[i][j] = k;
	    goto outofloopk_cc;
	  }
      }
      NC_Error_Exit_2(NCout, "Node of element does not exist in the connectivity list!", "Number of element = ", state().extNumbersOfElements[i], "Local number of the node in the element = ", j);
    outofloopk_cc:
      continue;
    }
  }

  
  
  
  if( check_normals)
    if( !NC_checkedges() )
      NC_Error_Exit_0(NCout, "There seems to be at least one element that is differently ordered to the rest");
  

  
  
  
  
  Matrix<double>crdel(NNODPE, NDIM);
  Vector<double> crdpoi(NDIM), shfunx(NNODPE), elnorv(NDIM), shnds(NNODPE),
    shndt(NNODPE), dxds(NDIM), dxdt(NDIM);
  
  
  state().averageElementArea_ = areevl = 0.0;
  int nbouel = 0, inode, nevlel = 0;
  double s, t, are0 = 0.0, leng;
  
  for(i=0; i<state().numElements_; i++)
    { checkpoint(); 
      
      if(state().listElementProperty[i] < 0 || state().listElementProperty[i] > 2 || state().listElementProperty[i] == 1)
	NC_Error_Exit_2(NCout, "state().listElementProperty[i] must be equal to 0 or 2!",
			"number of the element = ", state().extNumbersOfElements[i], "state().listElementProperty[i] = ", state().listElementProperty[i]);
      
      inode = state().listNumberNodesPerElement[i];
      
      for(j=0; j<inode; j++) for(k=0; k<NDIM; k++) crdel(j, k) = state().nodesCoordinates[state().elementsConnectivity[i][j]][k];
      
      switch(inode)
	{
	case 3:
	  s = t = 1.0/3.0;
	  are0 = 0.5;
	  break;
	case 4:
	  s = t = 0.0;
	  are0 = 4.0;
	  break;
	default: NC_Error_Exit_2(NCout, "Number of nodes of an element must be 3 or 4!",
				 "number of nodes of the element = ", inode,
				 "number of the element = ", state().extNumbersOfElements[i]);
	}
      
      
      NC_ComputeGlobalCoordinates(crdpoi, shfunx, crdel, s, t, inode);
      leng = NC_ComputeUnitVectorOnElement(elnorv, shnds, shndt, dxds, dxdt, crdel, s, t, inode);
      
      for(k=0; k<NDIM; k++)
	{ checkpoint(); 
	  state().elenor[i][k] = elnorv[k];
	  state().centel[i][k] = crdpoi[k];
	}
      state().areael[i] = leng*are0;
      
      if(state().listElementProperty[i] == 2)
	{
	  areevl += state().areael[i];
	  nevlel++;
	}
      else
	{
	  state().averageElementArea_ += state().areael[i];
	  nbouel++;
	}
      
      for(j=0; j<inode; j++)
	{ checkpoint(); 
	  if(inode == NETYP3)
	    {
	      s = CSI6[j];
	      t = ETA6[j];
	    }
	  else
	    {
	      s = CSI8[j];
	      t = ETA8[j];
	    }
	  leng = NC_ComputeUnitVectorOnElement(elnorv, shnds, shndt, dxds, dxdt, crdel,
					       s, t, inode);
	  
	  for(k=0; k<NDIM; k++)
	    { checkpoint(); 
	      state().elenor[i][(j + 1)*NDIM + k] = elnorv[k];
	    }
	}
    } 
  
  state().averageElementArea_ /= (double)nbouel;
  if(nevlel > 0) areevl /= (double)nevlel;
  
  
  
  
  for(i=0; i<state().numNodes_; i++) state().isNodeMeshOrEvalNode[i] = -3;
  
  for(i=0; i<state().numElements_; i++)
    { checkpoint(); 
      if(state().listElementProperty[i] == 0)
	{
	  for(j=0; j<state().listNumberNodesPerElement[i]; j++) state().isNodeMeshOrEvalNode[state().elementsConnectivity[i][j]] = 1;
	}
    }
  
  for(i=0; i<state().numElements_; i++)
    { checkpoint(); 
      if(state().listElementProperty[i] == 2)
	{
	  for(j=0; j<state().listNumberNodesPerElement[i]; j++)
	    { checkpoint(); 
	      if(state().isNodeMeshOrEvalNode[state().elementsConnectivity[i][j]] != 1)
		{
		  state().isNodeMeshOrEvalNode[state().elementsConnectivity[i][j]] = -1;
		}
	      else NC_Error_Exit_1(NCout, "BE mesh and evaluation mesh have common node!",
				   "Number of the common node = ", state().extNumbersOfNodes[state().elementsConnectivity[i][j]]);
	    }
	}
    }
  
  state().numNodesOfBoundaryMesh_ = state().numNodesOfEvaluationMesh_ = 0;
  for(i=0; i<state().numNodes_; i++)
    { checkpoint(); 
      if(state().isNodeMeshOrEvalNode[i] == 1)
	{
	  state().isNodeMeshOrEvalNode[i] = state().numNodesOfBoundaryMesh_;
	  state().numNodesOfBoundaryMesh_++;
	}
      else if(state().isNodeMeshOrEvalNode[i] == -1)
	{
	  state().numNodesOfEvaluationMesh_++;
	}
      else if(state().isNodeMeshOrEvalNode[i] == -3)
	{
	  runtime->quiet << "\n" << "number of the isolated node: " << state().extNumbersOfNodes[i] << endl;
	  NCout << "\n" << "number of the isolated node: " << state().extNumbersOfNodes[i] << endl;
	  NC_Error_Exit_0(NCout, "Isolated node found!");
	}
    }
  
  if(state().numNodesOfBoundaryMesh_ == 0)
    NC_Error_Exit_0(NCout, "Number of boundary elements is zero!");
  
  
  double crdmaxmin[3][2], dtl = sqrt(state().averageElementArea_)/100.0;
  
  for(j=0; j<NDIM; j++)
    { checkpoint(); 
      crdmaxmin[j][0] = -9.9e20;
      crdmaxmin[j][1] = 9.9e20;
    }
  for(i=0; i<state().numNodes_; i++)
    { checkpoint(); 
      if(state().isNodeMeshOrEvalNode[i] < 0) continue;
      for(j=0; j<NDIM; j++)
	{ checkpoint(); 
	  if(state().nodesCoordinates[i][j] > crdmaxmin[j][0]) crdmaxmin[j][0] = state().nodesCoordinates[i][j];
	  if(state().nodesCoordinates[i][j] < crdmaxmin[j][1]) crdmaxmin[j][1] = state().nodesCoordinates[i][j];
	}
    }
  
  for(j=0; j<NDIM; j++) { checkpoint(); 
    if(state().numSymmetricPlanes_ == 0) {
      crdmaxmin[j][0] -= crdmaxmin[j][1];
    } else {
      if(state().listSymmetryPlanes[j] == 0) {
	crdmaxmin[j][0] -= crdmaxmin[j][1];
      } else if(abs(state().listSymmetryPlanes[j]) <= 2) {
	if(crdmaxmin[j][0] >= state().listSymmetryPlanesCoordinates[j]-dtl && crdmaxmin[j][1] >= state().listSymmetryPlanesCoordinates[j]-dtl){
	  crdmaxmin[j][0] = 2.0*(crdmaxmin[j][0] - state().listSymmetryPlanesCoordinates[j]);
	} else if(crdmaxmin[j][0] <= state().listSymmetryPlanesCoordinates[j]+dtl && crdmaxmin[j][1] <= state().listSymmetryPlanesCoordinates[j]+dtl) {
	  crdmaxmin[j][0] = 2.0*(state().listSymmetryPlanesCoordinates[j] - crdmaxmin[j][1]);
	} else {
	  NC_Error_Warning_1(NCout, "", "Number of the symmetric plane = ", j);
	  NC_Error_Exit_2(NCout,
			  "All BE nodes must be located on the same side of a symmetric plane!",
			  "crdmaxmin[j][0] - state().listSymmetryPlanesCoordinates[j] = ", crdmaxmin[j][0] - state().listSymmetryPlanesCoordinates[j],
			  "crdmaxmin[j][1] - state().listSymmetryPlanesCoordinates[j] =  = ", crdmaxmin[j][1] - state().listSymmetryPlanesCoordinates[j]);
	}
      } else {
	NC_Error_Exit_2(NCout, "state().listSymmetryPlanes[j] mist be 0, -1, 1, -2, 2!", "state().listSymmetryPlanes[j] = ", state().listSymmetryPlanes[j],
			"j = ", j);
      }
    }
  }
  state().diameterBE_ = 0.0;
  for(j=0; j<NDIM; j++) state().diameterBE_ += crdmaxmin[j][0]*crdmaxmin[j][0];
  state().diameterBE_ = sqrt(state().diameterBE_);
  
  
  int ielgrp = 0;
  for(i=0; i<state().numElements_; i++)
    { checkpoint(); 
      for(j=0; j<ielgrp; j++)
	{ checkpoint(); 
	  if(state().listElementsElementGroup[i] == state().indexOfElementGroup[j]) goto EndOfLoopI_Elgr;
	}
      state().indexOfElementGroup[ielgrp] = state().listElementsElementGroup[i];
      state().propertyOfGroup[ielgrp] = state().listElementProperty[i];
      ielgrp++;
    EndOfLoopI_Elgr: continue;
    }
  
  if(ielgrp < state().numElementGroups_) state().numElementGroups_ = ielgrp;
  else if(ielgrp > state().numElementGroups_)
    NC_Error_Exit_2(NCout, "ielgrp must be less than or equal to nelgrp", "ielgrp = ",
		    ielgrp, "nelgrp = ", state().numElementGroups_);
  
  
  for(j=0; j<state().numElementGroups_; j++) state().numberOfElementsInGroup[j] = 0;
  for(i=0; i<state().numElements_; i++)
    { checkpoint(); 
      for(j=0; j<state().numElementGroups_; j++)
	{ checkpoint(); 
	  if(state().listElementsElementGroup[i] == state().indexOfElementGroup[j])
	    {
	      state().numberOfElementsInGroup[j]++;
	      break;
	    }
	}
    }
}



bool NC_checkedges(void) {
  
  
  
  
  
  
  
  
  
  
  
  
  
  

  Vector<int> hedges(state().numNodes_);
  int nvert, iel, first, second, j;
  bool meshokay;
  meshokay = true;
  for (j = 0; j < state().numNodes_; j++)
    hedges[j] = 0;
  for(iel = 0; iel < state().numElements_; iel++) { checkpoint(); 
    if( state().listElementProperty[iel] == 2 ) continue;  
    nvert = state().listNumberNodesPerElement[iel];
    
    for (j = 0; j < nvert - 1; j++) { checkpoint(); 
      first = state().elementsConnectivity[iel][j];
      second = state().elementsConnectivity[iel][j+1];
	  
      
      hedges[first] -= second + 1;
      hedges[second] += first + 1;
    }
    
    hedges[ state().elementsConnectivity[iel][nvert - 1] ] -= state().elementsConnectivity[iel][0] + 1;
    hedges[ state().elementsConnectivity[iel][0] ] += state().elementsConnectivity[iel][nvert - 1] + 1;
  }


  
  for (j = 0; j < state().numNodes_; j++) { checkpoint(); 
    if( hedges[j] != 0 ) {
      runtime->quiet << "Problem with element containing the node " << state().extNumbersOfNodes[j] << "\n";
      meshokay = false;
    }
  }
  return(meshokay);
}  


void NC_ReadBoundaryConditions
(
	ofstream& NCout,
	FILE* arg_inputFile_,				
	char* chinpline,			
	string chterms[NTRM],		
	int& maxbcline,			
	int& nbcline,				
	inputLineBoundaryCondition *bouconlin	
)
{
    int i, j, nterms, j_imag_p, j_imag_n, j_nega;

    
    while(!NC_GetLine(arg_inputFile_, chinpline, chterms));
    if(chterms[0].compare("BOUNDARY")) NC_Error_Exit_0(NCout, "Key word BOUNDARY expected!");

    
    nbcline = -1;
    for(i=0; i<maxbcline; i++)
    { checkpoint(); 
        nterms = 0;
        while(!nterms){ checkpoint(); nterms = NC_GetLine(arg_inputFile_, chinpline, chterms);}

        if(!chterms[0].compare("RETU"))
        {
            nbcline = i;
            break;
        }
        else
        {	
			
			
			
			
			
			
			
			
			
			
            if(nterms < 9) NC_Error_Exit_1(NCout, "A input line under BOUNDARY is too short!",
                                           "Number of the input line = ", i);
            if(chterms[0].compare("ELEM")) NC_Error_Exit_0(NCout, "Key word ELEM expected!");
            bouconlin[i].nLow = NC_String2Integer(chterms[1]);
            if(chterms[2].compare("TO")) NC_Error_Exit_0(NCout, "Key word TO expected!");
            bouconlin[i].nHigh = NC_String2Integer(chterms[3]);

			
            bouconlin[i].sKeyword = chterms[4];

			
			if ( (chterms[6] == "-1" && chterms[8] != "-1") ||
			     (chterms[6] != "-1" && chterms[8] == "-1")){
				NC_Error_Exit_0(NCout,
					"ERROR: Curves have to be specified for all parts of a "
					"Boundary or Source. I.e., ALL curve numbers in one line "
					"must be -1 or ALL curve numbers must not be -1");
			}

			
			bouconlin[i].dReal = NC_String2Double(chterms[5]);
			if (chterms[6] == "-1"){
				
				bouconlin[i].bRef = false;
			} else {
				
				bouconlin[i].bRef = true;
				bouconlin[i].nRealRef = NC_String2Integer(chterms[6]);
			}
			
			bouconlin[i].dImag = NC_String2Double(chterms[7]);
			if (chterms[8] == "-1"){
				
				bouconlin[i].bRef = false;
			} else {
				
				bouconlin[i].bRef = true;
                bouconlin[i].nImagRef = NC_String2Integer(chterms[8]);
			}
			
			
			bouconlin[i].bNega = false;

        } 
    } 

    if(nbcline == -1) NC_Error_Exit_0(NCout, "Key word RETU expected!");
}



void NC_ReadSoundSources
(
	ofstream& NCout,
	FILE* arg_inputFile_,			
	char* chinpline,		
	string chterms[NTRM]	
)
{
	int i, nterms;
	double dw, re, im;

	
	if(state().numIncidentPlaneWaves_ > 0)
	{
		while(!NC_GetLine(arg_inputFile_, chinpline, chterms));
		if(chterms[0].compare("PLANE")) NC_Error_Exit_0(NCout, "Key word PLANE WAVE expected!");

		for(i=0; i<state().numIncidentPlaneWaves_; i++)
		{ checkpoint(); 
			nterms = 0;
			while(!nterms){ checkpoint(); nterms = NC_GetLine(arg_inputFile_, chinpline, chterms);}

			if(nterms != 8)
				NC_Error_Exit_2(NCout,
				"Number of terms of an input line for plane wave must be 8!",
				"Number of terms = ", nterms,
				"Number of the input line under the key word PLANE WAVE = ", i+1);

			state().numinw[i] = NC_String2Integer(chterms[0]);
			state().dirinw[i][0] = NC_String2Double(chterms[1]);
			state().dirinw[i][1] = NC_String2Double(chterms[2]);
			state().dirinw[i][2] = NC_String2Double(chterms[3]);
			Vcnorm_dim3_(state().dirinw[i], dw);
			re = NC_String2Double(chterms[4]);
			im = NC_String2Double(chterms[6]);
			state().zinwao[i].set(re, im);
			state().inwacurv[i][0] = NC_String2Integer(chterms[5]);
			state().inwacurv[i][1] = NC_String2Integer(chterms[7]);
		}
	}

	if(state().numPointSources_ > 0)
	{
		while(!NC_GetLine(arg_inputFile_, chinpline, chterms));
		if(chterms[0].compare("POINT")) NC_Error_Exit_0(NCout, "Key word POINT SOURCES expected!");

		for(i=0; i<state().numPointSources_; i++)
		{ checkpoint(); 
			nterms = 0;
			while(!nterms) { checkpoint(); nterms = NC_GetLine(arg_inputFile_, chinpline, chterms);}

			if(nterms != 8)
				NC_Error_Exit_2(NCout,
				"Number of terms of an input line for pont source must be 8!",
				"Number of terms = ", nterms,
				"Number of the input line under the key word POINT SOURCE = ", i+1);

			state().numposo[i] = NC_String2Integer(chterms[0]);
			state().coorps[i][0] = NC_String2Double(chterms[1]);
			state().coorps[i][1] = NC_String2Double(chterms[2]);
			state().coorps[i][2] = NC_String2Double(chterms[3]);
			re = NC_String2Double(chterms[4]);
			im = NC_String2Double(chterms[6]);
			state().zposoo[i].set(re, im);
			state().posocurv[i][0] = NC_String2Integer(chterms[5]);
			state().posocurv[i][1] = NC_String2Integer(chterms[7]);
		}
	}
}


void NC_ReadFrequencyCurves
(
	ofstream& NCout,
	FILE* arg_inputFile_,			
	char* chinpline,		
	string chterms[NTRM]	
)
{
    int i, j, nterms, mpcurv;
    bool bifrdcurv = false;

    
    state().ibvcurv = allocate_array<int*>((state().numElements_));
    for(i=0; i<state().numElements_; i++) state().ibvcurv[i] = allocate_array<int>((6));
    for(i=0; i<state().numElements_; i++) for(j=0; j<6; j++) state().ibvcurv[i][j] = -1;

    
    while(!NC_GetLine(arg_inputFile_, chinpline, chterms));
    if(!chterms[0].compare("CURVES"))
    {
        bifrdcurv = true;
    }
    else if(chterms[0].compare("POST"))
    {
        NC_Error_Exit_0(NCout, "Key word CURVES or POST PROCESS expected!");
    }

    
    state().numCurvesFrequency_ = mpcurv = 0;
    if(bifrdcurv)
    {
        nterms = 0;
        while(!nterms){ checkpoint(); nterms = NC_GetLine(arg_inputFile_, chinpline, chterms);}

        if(nterms != 2) NC_Error_Exit_1(NCout,
                                        "Number of terms of the input line direct under the key word CURVE must be 2!",
                                        "Number of terms = ", nterms);
        state().numCurvesFrequency_ = NC_String2Integer(chterms[0]);
        mpcurv = NC_String2Integer(chterms[1]);
    }
    if(state().numCurvesFrequency_ < 0) state().numCurvesFrequency_ = 0;
    if(mpcurv < 0) mpcurv = 0;

    
    state().nucurv = allocate_array<int>((state().numCurvesFrequency_));
    state().npcurv = allocate_array<int>((state().numCurvesFrequency_));
    state().frqcurv = allocate_array<double*>((state().numCurvesFrequency_));
    for(i=0; i<state().numCurvesFrequency_; i++) state().frqcurv[i] = allocate_array<double>((mpcurv));
    state().faccurv = allocate_array<double*>((state().numCurvesFrequency_));
    for(i=0; i<state().numCurvesFrequency_; i++) state().faccurv[i] = allocate_array<double>((mpcurv));

    
    if(bifrdcurv)
    {
        for(i=0; i<state().numCurvesFrequency_; i++)
        { checkpoint(); 
            nterms = 0;
            while(!nterms){ checkpoint(); nterms = NC_GetLine(arg_inputFile_, chinpline, chterms);}

            if(nterms != 2) NC_Error_Exit_2(NCout,
                                            "Number of terms of the firs input line for a curve must be 2!",
                                            "Number of terms = ", nterms, "number of the curve = ", i);
            state().nucurv[i] = NC_String2Integer(chterms[0]);
            state().npcurv[i] = NC_String2Integer(chterms[1]);

            for(j=0; j<state().npcurv[i]; j++)
            { checkpoint(); 
                while(!NC_GetLine(arg_inputFile_, chinpline, chterms));
                state().frqcurv[i][j] = NC_String2Double(chterms[0]);
                state().faccurv[i][j] = NC_String2Double(chterms[1]);
            }
        }
    }

    
    if(bifrdcurv)
    {
        while(!NC_GetLine(arg_inputFile_, chinpline, chterms));
        if(chterms[0].compare("POST")) NC_Error_Exit_0(NCout, "Key word POST PROCESS expected!");
    }

    
    nterms = 0;
    while(nterms == 0 || nterms > 1 || (nterms == 1 && chterms[0].compare("END")))
    { checkpoint(); 
        nterms = NC_GetLine(arg_inputFile_, chinpline, chterms);
    }
}


void NC_StoreBoundaryConditions
(
	ofstream& NCout,
	FILE* arg_inputFile_,				
	char* chinpline,			
	string chterms[NTRM],		
	int& nbcline,				
	inputLineBoundaryCondition *bouconlin	
)
{
    int i, i1, j, j1, k, nbtyp = 0, nrearefp = 0, nimgrefp = 0, nrearefn = 0, nimgrefn = 0,
    maxnuel = 0;
    string keywd;
    bool ifimpe, ifnega, ifref;
    double reap, rean = 0.0, imgp = 0.0, imgn = 0.0, dwk;
    Vector<int> ifimpecurve(state().numCurvesFrequency_);
    if(state().numCurvesFrequency_ > 0) for(i=0; i<state().numCurvesFrequency_; i++) ifimpecurve[i] = 0;
    int *ElGlobToLoca;


    
    for(i=0; i<state().numElements_; i++) if(state().extNumbersOfElements[i] > maxnuel) maxnuel = state().extNumbersOfElements[i];
    if(maxnuel > 10000000)
    {
        NCout << "\nWarning: external element number too large!" << endl;
        NCout << "The maximun value = " << maxnuel << endl;
        runtime->quiet << "\nWarning: external element number too large!" << endl;
        runtime->quiet << "The maximun value = " << maxnuel << endl;
    }
    maxnuel++;
    ElGlobToLoca = allocate_array<int>((maxnuel));
    for(i=0; i<maxnuel; i++) ElGlobToLoca[i] = -1;

    for(i=0; i<state().numElements_; i++)
    { checkpoint(); 
        if(state().listElementProperty[i] == 2) continue;

        i1 = state().extNumbersOfElements[i];
        for(j=0; j<state().numElements_; j++)
        { checkpoint(); 
            if(state().listElementProperty[j] == 2) continue;
            if(state().extNumbersOfElements[j] == i1)
            {
                ElGlobToLoca[i1] = j;
                break;
            }
        }
    }

    
    for(i=0; i<state().numElements_; i++)
    { checkpoint(); 
        state().ibval[i] = 0;
        for(j=0; j<NNODPE; j++) state().zbvao0[i][j].set(0.0, 0.0);
        state().zbvao1[i].set(0.0, 0.0);
        state().zbvao2[i].set(0.0, 0.0);
    }

    
    for(i=0; i<nbcline; i++)
    { checkpoint(); 
        keywd = bouconlin[i].sKeyword;
        ifimpe = false;

        if(keywd == "VELO")
        {
            nbtyp = 0;
        }
        else if(keywd == "PRES")
        {
            nbtyp = 1;
        }
        else if(keywd == "ADMI")
        {
            nbtyp = 2;
        }
        else if(keywd == "IMPE")
        {
            nbtyp = 2;
            ifimpe = true;
        }
        else if(keywd == "TRAD")
        {
            nbtyp = 3;
            NC_Error_Exit_0(NCout, "Transfer admittance boundary condition is not supported!");
        }
        else if(keywd == "TRIM")
        {
            nbtyp = 3;
            ifimpe = true;
            NC_Error_Exit_0(NCout, "Transfer impedance boundary condition is not supported!");
        }
        else
        {
            runtime->quiet << "The false key word = " << keywd << endl;
            NCout << "The false key word = " << keywd << endl;
            NC_Error_Exit_1(NCout, "False key word in a input line for boundary conditions!",
                            "Input line number = ", i + 1);
        }

        
        ifref = bouconlin[i].bRef;
        ifnega = bouconlin[i].bNega;
        reap = bouconlin[i].dReal;
        imgp = bouconlin[i].dImag;

	if(ifimpe) {
	  if(reap == 0 && imgp == 0) {
	    ifimpe = false;
	    nbtyp = 1;  
	  }
	}

	if(ifref && ifimpe) {
	  runtime->quiet << "Sorry Impedance boundary conditions in CURVES is not implemented yet. Please use ADMI conditions. They work with CURVES. Sorry for the inconvinience.\n";
	  fail_exit(-1);
	}

        if(ifimpe)
        {
            dwk = reap*reap + imgp*imgp;
            reap /= dwk;
            imgp /= (-dwk);
        }
        if(ifref)
        {
            nrearefp = bouconlin[i].nRealRef;
            nimgrefp = bouconlin[i].nImagRef;
        }
        if(ifnega)
        {
            rean = bouconlin[i].dNegReal;
            imgn = bouconlin[i].dNegImag;
            if(ifimpe)
            {
                dwk = rean*rean + imgn*imgn;
                rean /= dwk;
                imgn /= (-dwk);
            }
            if(ifref)
            {
                nrearefn = bouconlin[i].nNegRealRef;
                nimgrefn = bouconlin[i].nNegImagRef;
            }
        }

        
        for(j=bouconlin[i].nLow; j<=bouconlin[i].nHigh; j++)
        { checkpoint(); 
            j1 = ElGlobToLoca[j]; 
            if(j1 == -1) continue; 
            switch(nbtyp)
            {
                case 0:
                    for(k=0; k<state().listNumberNodesPerElement[j1]; k++) state().zbvao0[j1][k].set(reap, imgp);
                    break;
                case 1:
                    for(k=0; k<state().listNumberNodesPerElement[j1]; k++) state().zbvao0[j1][k].set(reap, imgp);
                    state().ibval[j1] = nbtyp;
                    break;
                case 2:
                    state().zbvao1[j1].set(reap, imgp);
                    if(ifnega) state().zbvao2[j1].set(rean, imgn);
                    if(ifref)
                    {
                        state().ibvcurv[j1][2] = nrearefp;
                        state().ibvcurv[j1][3] = nimgrefp;
                        if(ifnega)
                        {
                            state().ibvcurv[j1][4] = nrearefn;
                            state().ibvcurv[j1][5] = nimgrefn;
                        }
                    }
                    switch(state().ibval[j1])
                {
                    case 0:
                        state().ibval[j1] = nbtyp;
                        break;
                    case 1:
                        runtime->quiet << "Warning; for el. " << j << " the pressure boundary condition is " <<
                        "prescribed, the additional admittance boundary condition is ignored!" <<
                        endl;
                        break;
                    case 3:
                        state().ibval[j1] = 4;
                        break;
                    case 2:
                    case 4:
                        break;
                }
                    break;
                case 3:
                    for(k=0; k<state().listNumberNodesPerElement[j1]; k++) state().zbvao0[j1][k].set(reap, imgp);
                    if(ifref)
                    {
                        state().ibvcurv[j1][0] = nrearefp;
                        state().ibvcurv[j1][1] = nimgrefp;
                    }
                    if(state().ibval[j1] <= 1)
                    {
                        state().ibval[j1] = nbtyp;
                    }
                    else if(state().ibval[j1] == 2)
                    {
                        state().ibval[j1] = 4;
                    }
                    break;
            }
        } 

        
        if(ifimpe  && ifref)
        {
            ifimpecurve[nrearefp] = 1;
            ifimpecurve[nimgrefp] = 1;
            if(ifnega)
            {
                ifimpecurve[nrearefn] = 1;
                ifimpecurve[nimgrefn] = 1;
            }
        }
    } 

    
    for(i=0; i<state().numCurvesFrequency_; i++)
    { checkpoint(); 
        if(ifimpecurve[i])
        {
            for(j=0; j<state().npcurv[i]; j++)
            { checkpoint(); 
                if(fabs(state().faccurv[i][j]) > 1.0e-10)
                {
                    state().faccurv[i][j] = 1.0/state().faccurv[i][j];
                }
                else
                {
                    state().faccurv[i][j] = state().faccurv[i][j] >= 0.0 ? 1.0e10 : -1.0e10;
                }
            }
        }
    }

    release_array(ElGlobToLoca);
}


void NC_WriteParameters
(
	ofstream& NCout,
	double* Freqs
 )
{
    int i, j, j1, k, k1, numFrequencies_ori = state().numFrequencies_;
    double freq_max, dmesh_ave= 0.0, dmesh_eqi = 0.0, dmesh_max = 0, dmesh_min = 1.0e20,
    dist, dis_ref;

    
    k1 = 0;
    for(i=0; i<state().numElements_; i++)
    { checkpoint(); 
        j1 = state().listNumberNodesPerElement[i];

        if(j1 != NETYP3 && j1 != NETYP4) NC_Error_Exit_1(NCout, "An element must have 3 or 4 nodes!",
                                                         "Number of the false element = ", state().extNumbersOfElements[i]);

        if(state().listElementProperty[i] == 2) continue;

        if(j1 > NETYP4) j1 /= 2;
        if(state().areael[i] < 0.0) NC_Error_Exit_2(NCout, "Element with negative area is found!",
                                            "external elemen number = ", state().extNumbersOfElements[i], "internal element number = ", i);
        dis_ref = sqrt(state().areael[i]);
        if(j1 == NETYP3) dis_ref *= 1.51967;
        for(j=0; j<j1; j++)
        { checkpoint(); 
            k = j + 1;
            if(k == j1) k = 0;
            dist = Dispoi_dim3_(state().nodesCoordinates[state().elementsConnectivity[i][j]], state().nodesCoordinates[state().elementsConnectivity[i][k]]);
            if(dist > dmesh_max) dmesh_max = dist;
            if(dist < dmesh_min) dmesh_min = dist;
            dmesh_ave += dist;
            if(j1 == NETYP3) {dmesh_eqi += dist*0.748;} else {dmesh_eqi += dist;}
            if(dist > dis_ref*10.0 && dist < dis_ref*100.0) NC_Error_Warning_2(NCout,
                                                                               "Element with very long side found!", "external elemen number = ",
                                                                               state().extNumbersOfElements[i], "internal element number = ", i);
            if(dist > dis_ref*100.0) NC_Error_Exit_2(NCout,
                                                     "Element of very distorted form is found!", "external elemen number = ",
                                                     state().extNumbersOfElements[i], "internal element number = ", i);
            k1++;
        }
    }
    dmesh_ave /= (double)k1;
    dmesh_eqi /= (double)k1;

    state().delta_ = dmesh_min/100.0;

    

    double ele_leng = dmesh_eqi, elnumperwve;

    elnumperwve = 6;

    freq_max = state().speedOfSound_/(elnumperwve*ele_leng);

    for(i=0; i<state().numFrequencies_; i++)
    { checkpoint(); 
        if(Freqs[i] > freq_max) break;
    }

    

    
    if(numFrequencies_ori > i)
    {
        NCout << endl;
        NC_Error_Warning_2(NCout, "There are some frequencies higher than the highest allowable frequency!",
			   "Number of frequencies outside the range= ", numFrequencies_ori - i,
                           "The highest allowable frequency = ", freq_max);
    }

    
    NCout << state().versionNumber_ << endl;
    NCout << state().jobTitle_ << endl;

    NCout << endl;
    NCout << "Number of frequency steps ................... = " << state().numFrequencies_ << endl;

    NCout << endl;
    NCout << "First step .................................. = " << state().firstFrequencyStep_ << endl;
    NCout << "Number of steps ............................. = " << state().numFrequencies_ << endl;
    NCout << "Step increment .............................. = " << state().frequencyStepIncrement_ << endl;
    NCout << "Highest allowable frequency .................. = " << freq_max << endl;

    
    NCout << "\n\n\n>> B A S I C   P A R A M E T E R S   F O R   B E M <<\n" << endl;

    NCout << "Number of element groups .................... = " << state().numElementGroups_ << endl;
    NCout << "Total number of elements .................... = " << state().numElements_ << endl;
    NCout << "Number of boundary elements ................. = " << state().numElementsOfBoundaryMesh_ << endl;
    NCout << "Number of evaluation elements ............... = " << state().numElementsOfEvaluationMesh_ << endl;
    NCout << "Maximum edge length of boundary elements .... = " << dmesh_max << endl;
    NCout << "Average edge length of boundary elements .... = " << dmesh_ave << endl;
    NCout << "Minimum edge length of boundary elements .... = " << dmesh_min << endl;

    NCout << endl;
    NCout << "Total number of nodes ....................... = " << state().numNodes_ << endl;
    NCout << "Number of nodes of the boundary elements .... = " << state().numNodesOfBoundaryMesh_ << endl;
    NCout << "Number of nodes of the evaluation elements .. = " << state().numNodesOfEvaluationMesh_ << endl;

    NCout << endl;
    NCout << "Number of symmetric planes .................. = " << state().numSymmetricPlanes_ << endl;

    NCout << endl;
    NCout << "Number of incident plane waves .............. = " << state().numIncidentPlaneWaves_ << endl;
    NCout << "Number of incident spharic waves ............ = " << state().numPointSources_ << endl;

    NCout << endl;
    NCout << "Sound speed ................................. = " << state().speedOfSound_ << endl;
    NCout << "Density of the medium ....................... = " << state().densityOfMedium_ <<
    endl;

    NCout << endl;
    NCout << "Diameter of the BE-mesh ..................... = " << state().diameterBE_ << endl;

    if(state().methodBEM_ > 0) {
        NCout << endl;
        NCout << "Minimum number of terms in the FMM expansion. = " << state().minExpansionTermsFMM_
        << endl;
    }

    NCout << endl;

    if(state().isInternalProblem_ == 0) NCout << "Exterior domain problem" << endl;
    if(state().isInternalProblem_ == 1) NCout << "Interior domain problem" << endl;

    NCout << endl;
    if(state().harmonicTimeFactor_ == 1.0)
    {
        NCout << "Harmonic time factor = EXP(-i*omega*t)" << endl;
    }
    else
    {
        NCout << "Harmonic time factor = EXP(i*omega*t)" << endl;
    }

    NCout << endl;

    switch(state().methodSolver_)
    {
        case 0:
            NCout << "state().methodSolver_  = 0: CGS iterative solver" << endl;
            break;
        case 4:
            NCout << "state().methodSolver_  = 4: Gaussean elimination (direct) solver" << endl;
            break;
        default:
            NC_Error_Exit_0(NCout, "state().methodSolver_ must be equal to 0, 1, 2, 3 or 4!");
    }
    if(state().methodSolver_ < 4)
    {
        switch(state().methodPreconditioner_)
        {
            case 0:
                NCout << "state().methodPreconditioner_ = 0: incomplete LU-decomposition preconditioner"
                << endl;
                break;
            case 1:
                NCout << "state().methodPreconditioner_ = 1: scaling preconditioner" << endl;
                break;
            case 2:
                NCout << "state().methodPreconditioner_ = 2: no preconditioner" << endl;
                break;
            default:
                NC_Error_Exit_0(NCout, "state().methodPreconditioner_ must be equal to 0, 1 or 2!");
        }
    }

    
    double ell = sqrt(state().averageElementArea_);

    NCout << endl;
    NCout << " No.   Freq.   Lambda/L_BE_Mesh  Lambda/L_Element" << endl;

    for(i=0; i<state().numFrequencies_; i++)
    { checkpoint(); 
        double wavl = state().speedOfSound_/Freqs[i];
        NCout << setw(3) << i+1 << " " << setw(7) << Freqs[i]
        << "   " << setw(10) << wavl/state().diameterBE_
        << "        " << setw(10) << wavl/ell << endl;
    }
    NCout << endl;
}


int NC_GetLine
(
	FILE *arg_inputFile_,			
	char *chinpline,		
	string chterms[NTRM]	
)
{

 if(!fgets(chinpline,SIZE,arg_inputFile_))throw Failure(1,"Unexpected end of acoustic input");
 std::string line(chinpline);
 if(line.size()>=SIZE-1 && line.back()!='\n')throw Failure(1,"Acoustic input line exceeds bound");
 for(int i=0;i<NTRM;++i)chterms[i].clear();
 std::istringstream tokens(line);tokens.imbue(std::locale::classic());
 std::string token;int count=0;
 while(tokens>>token){ checkpoint(); 
  if(token.find('#')!=std::string::npos)break;
  if(count==NTRM)throw Failure(1,"Too many acoustic input fields");
  chterms[count++]=token;
 }
 return count;

}



int NC_String2Integer
(
	string Ori_Str		
)
{

 char *end=nullptr;errno=0;const char *text=Ori_Str.c_str();
 long value=std::strtol(text,&end,10);
 if(errno==ERANGE || end==text || *end!='\0' || value<std::numeric_limits<int>::min() || value>std::numeric_limits<int>::max())throw Failure(1,"Invalid bounded acoustic integer");
 return int(value);

}



double NC_String2Double
(
	string Ori_Str		
)
{

 char *end=nullptr;errno=0;const char *text=Ori_Str.c_str();
 double value=std::strtod(text,&end);
 if(errno==ERANGE || end==text || *end!='\0' || !std::isfinite(value))throw Failure(1,"Invalid finite acoustic number");
 return value;

}


void NC_ComputeFrequencies
(
	ofstream& NCout,
	int& npoicurv,			
	double* p_dcurvfrq,		
	double* p_dcurvtime,	
	double* Freqs			
)
{
	int i, j, j1;
	double t;

	for(i=0; i<state().numFrequencies_; i++)
	{ checkpoint(); 
		t = state().firstFrequencyStep_ + (double)(i + 1)*state().frequencyStepIncrement_;
		if(p_dcurvtime[0] >= t)
		{
			Freqs[i] = p_dcurvfrq[0];
			continue;
		}
		j1 = -1;
		for(j=1; j<npoicurv; j++)
		{ checkpoint(); 
			if(p_dcurvtime[j] > t)
			{
				j1 = j;
				break;
			}
			else if(p_dcurvtime[j] == t)
			{
				Freqs[i] = p_dcurvfrq[j];
				j1 = npoicurv;
				break;
			}
		}
		if(j1 == -1)
		{
			Freqs[i] = p_dcurvfrq[npoicurv - 1];
		}
		else if(j1 < npoicurv)
		{
			Freqs[i] = p_dcurvfrq[j1 - 1] +
				(t - p_dcurvtime[j1 - 1])*(p_dcurvfrq[j1] - p_dcurvfrq[j1 - 1])/
				(p_dcurvtime[j1] - p_dcurvtime[j1 - 1]);
		}
	}
}
