












#ifdef isWindows                                                                    

#else                                                                               


#endif                                                                              
using namespace std;                                                                
typedef unsigned int uint;                                                          




void NC_ControlProgram(ofstream&,int,bool,bool);
void NC_FrequencyInformations(ostream&, ofstream&);



extern void NC_Read(ofstream&,FILE *,char*,string[],double*,bool);
extern void NC_ReadBasicParametersA(ofstream&,FILE*,char*, string[]);
extern int NC_DeclareArrays(ofstream&,double*,bool);
extern void NC_AllocateSDTmtxsSLFMM(ofstream&);
void NC_AllocateSDTmtxsMLFMM(ofstream&);
extern void NC_DeleteArrays(const int&,const int&,const int&);
extern void NC_SetupEquationSystem(ofstream&);
extern void NC_UpdateFreqCurves(ofstream&,double*);
extern void NC_PostProcessing(ofstream&);



































































int embedded_main(int argc, char **argv)
{

  int i;          
  int iend = 0;   
  char filename[200];  
  bool estimate_ram = false; 
  bool check_normals = false; 
  state().istart_ = 0;    


  
  i = 1;
  while (i < argc) { checkpoint(); 
    if(!strcmp(argv[i],"-h")) {
      printf("-istart   int : start index of iteration\n");
      printf("-iend     int : end index of iteration\n");
      printf("-nitermax int : max number of CGS iterations\n");
      printf("-estimate_ram : estimation the RAM consumption of ML-FMM-BEM and write estimate to Memory.txt. Estimate is obtained from the number of non-zeros in the FMM matrices.\n");
      printf("-check_normals : check if all normals point to the same domain\n");
      printf("-adapt_fmmlength: the truncation parameter of the multipole expansion is adapted to the radii of the cluster involved\n");
      printf("-h            : this message\n");
      fail_exit(0);
    }
    else if(!strcmp(argv[i],"-istart")) {
      i++;
      if(i<argc) {
	state().istart_=atoi(argv[i]);
      }
    }
    else if(!strcmp(argv[i],"-iend")) {
      i++;
      if(i<argc)
	iend=atoi(argv[i]);
    }
    else if(!strcmp(argv[i],"-nitermax")) {
      i++;
      if( i < argc )
	state().niter_max_ = atoi( argv[i] );
    }
    else if(!strcmp(argv[i],"-estimate_ram")) {
      estimate_ram = true;
    }
    else if(!strcmp(argv[i],"-check_normals")) {
      check_normals = true;
    }
    else if(!strcmp(argv[i],"-adapt_fmmlength")) {
      state().adapt_fmmlength_ = true;
    }
    else {
      runtime->quiet << "\nNumCalc was called with an unknown parameter or flag. Use NumCalc -h for help.\n";
      fail_exit(-1);
    }
    i++;
  }


  
  if( state().istart_ < 0 ) {
    runtime->quiet << "I need to start with a step bigger 0\n";
    fail_exit(-1);
  }

  

	
  time_t lot = time(NULL);
  tm *d_t = local_walltime(lot);

	
	runtime->quiet << "\n---------- NumCalc started: " << d_t->tm_mday << "/" << d_t->tm_mon + 1 <<
		"/" << d_t->tm_year + 1900 << " " << d_t->tm_hour << ":" << d_t->tm_min <<
		":" << d_t->tm_sec << " ----------\n" << endl;

#ifdef isWindows

#else
    char Name[150];
    memset(Name, 0, 150);
    
    runtime->quiet << "Running on:         " << Name << "\n" << endl;
#endif


    
    if(state().istart_ > 0) {
      if( iend >= state().istart_ )
	sprintf(filename,"NC%d-%d.out",state().istart_,iend);
      else if (iend > 0) {
	runtime->quiet << "istart is bigger than iend. Stopping now!\n";
	fail_exit(-1);
      }
      else
	sprintf(filename,"NCfrom%d.out",state().istart_);
    }
    else {
      if(iend > 0)
	sprintf(filename,"NCuntil%d.out",iend);
      else
	sprintf(filename,"NC.out");
    }

	ofstream NCout(local_path(filename));
	if(!NCout) NC_Error_Exit_0(NCout, "can not open the output stream NCout!");

	
    NCout << "\nStart time: " << d_t->tm_mday << "/" << d_t->tm_mon + 1 << "/" <<
        d_t->tm_year + 1900 << " " << d_t->tm_hour << ":" << d_t->tm_min << ":" <<
        d_t->tm_sec << endl;


      
      
      
#ifdef isWindows
      int ifmkd;
      if( (state().istart_ == 0 && iend == 0) && (!estimate_ram)) { 
	if(0==-1) runtime->quiet << "\nCannot create directory be.out";
	
      }
      ifmkd = local_mkdir("be.out"); 
      
#else
      if(state().istart_ == 0 && iend == 0 && !estimate_ram) {
	if(0==-1) runtime->quiet << "\nCannot create directory be.out";
	
      }
      local_mkdir("be.out");  
      
#endif

	
      NC_ControlProgram(NCout,iend,estimate_ram,check_normals);

	
	lot = time(NULL);
	d_t = local_walltime(lot);

	
    NCout << "\nEnd time: " << d_t->tm_mday << "/" << d_t->tm_mon + 1 << "/" <<
        d_t->tm_year + 1900 << " " << d_t->tm_hour << ":" << d_t->tm_min << ":" <<
        d_t->tm_sec << endl;

	runtime->quiet << "\n---------- NumCalc ended: " << d_t->tm_mday << "/" << d_t->tm_mon + 1 <<
		"/" << d_t->tm_year + 1900 << " " << d_t->tm_hour << ":" << d_t->tm_min <<
		":" << d_t->tm_sec << " ----------" << endl;

	return(0);
}


void NC_ControlProgram(ofstream& NCout,int iend, bool estimate_ram, bool check_normals)
{
  double *Freqs = nullptr;

  int i, j, l, nptsouc, ifdiff, terml, npthet, npphi, npsph;
  double rw, rd_max;
  double H_Argum_Min = 0.0; 

  
  string chterms[NTRM];
  
  char* chinpline = allocate_array<char>((SIZE));

  
  time_t ltim[7], lti_eqa = 0, lti_sol = 0, lti_pos = 0, lti_est, lti_sst, lti_pst;


  ofstream NCestim;

  if(estimate_ram) {
    NCestim.open(local_path("Memory.txt"));
    if(!NCestim)
      if(!NCout) NC_Error_Exit_0(NCout, "can not open the output stream NCestim!");
  }

  time(&ltim[0]);





  
  state().inputFile_ = local_open("NC.inp", "r");
  if(state().inputFile_) 
    {
      
      NCout << "\n\n>> G E N E R A L   P A R A M E T E R S <<\n" << endl;

      runtime->quiet << "3D analysis" << endl;
      NCout << "\n3D analysis" << endl;

      
      NC_ReadBasicParametersA(NCout, state().inputFile_, chinpline, chterms);

      
      Freqs = allocate_array<double>((state().numFrequencies_));

      
      NC_Read(NCout, state().inputFile_, chinpline, chterms, Freqs, check_normals);
    }
  else 
    {
      NC_Error_Exit_0(NCout, "No input file found!");
    }

  
  release_array(chinpline);

  
  nptsouc = abs(state().numPointSources_);

  time(&ltim[1]);

  
  if(state().minExpansionTermsFMM_ == 5) {H_Argum_Min = 0.0051;}
  else if(state().minExpansionTermsFMM_ == 6) {H_Argum_Min = 0.0177;}
  else if(state().minExpansionTermsFMM_ == 7) {H_Argum_Min = 0.0445;}
  else if(state().minExpansionTermsFMM_ == 8) {H_Argum_Min = 0.0905;}
  else if(state().minExpansionTermsFMM_ == 9) {H_Argum_Min = 0.1597;}
  else if(state().minExpansionTermsFMM_ == 10) {H_Argum_Min = 0.2547;}

  time(&ltim[2]);

  

  if(iend > 0)  {
    if(iend > state().numFrequencies_)
      iend = state().numFrequencies_;
  }
  if(iend == 0)
    iend = state().numFrequencies_;
  if(state().istart_ > 0)
    state().istart_--;



  for(state().currentFrequency_ = state().istart_; state().currentFrequency_ < iend; state().currentFrequency_++) { checkpoint(); 
frequency_progress(state().currentFrequency_, state().numFrequencies_);
    time(&ltim[3]);

    
    state().frequency_ = Freqs[state().currentFrequency_];					
    state().omega1_ = 2.0*PI*state().frequency_;				
    state().rpfact_ = state().densityOfMedium_*state().omega1_*state().harmonicTimeFactor_;		
    state().waveNumbers_ = state().omega1_/state().speedOfSound_;				
    state().waveLength_ = state().speedOfSound_/state().frequency_;				
    state().minClusterDistance_ = H_Argum_Min/state().waveNumbers_;        

    

    
    runtime->quiet << "\nStep " << state().currentFrequency_ + 1 << ", Frequency = " << state().frequency_ << " Hz" << endl;

    NCout << "\n\n\n>> S T E P   N U M B E R   A N D   F R E Q U E N C Y <<\n"
	  << endl;
    NCout << "Step " << state().currentFrequency_ + 1 << ", Frequency = " << state().frequency_ << " Hz"
	  << endl;

    
    ifdiff = NC_DeclareArrays(NCout, Freqs, estimate_ram);

    
    if(ifdiff) NC_FrequencyInformations(runtime->quiet, NCout);

    
    if(state().methodFMM_ >= 2 && state().currentFrequency_ > state().istart_)
      {
	state().ClustArray = state().clulevarry[state().nlevtop_].ClastArLv;

	state().numOriginalClusters_ = state().clulevarry[state().nlevtop_].nClustOLv;
	state().numOriginalReflectedClusters_ = state().clulevarry[state().nlevtop_].nClustSLv;

	state().numExpansionTerms_ = state().clulevarry[state().nlevtop_].nExpaTermLv;
	state().numIntegrationPointsUnitSphere_ = state().clulevarry[state().nlevtop_].nPoinSpheLv;
	state().maxClusterRadiusBE_ = state().clulevarry[state().nlevtop_].RadiMaxLv;
	state().avgClusterRadiusBE_ = state().clulevarry[state().nlevtop_].RadiAveLv;
	state().minClusterRadiusBE_ = state().clulevarry[state().nlevtop_].RadiMinLv;

	state().weisphe = state().clulevarry[state().nlevtop_].weisphe;
	state().uvcsphe = state().clulevarry[state().nlevtop_].uvcsphe;
      }

    

    
    NC_UpdateFreqCurves(NCout, Freqs);

    
    switch(state().methodFMM_) 
      {
      case 1: 
	
	rw = 2.0*state().maxClusterRadiusBE_*state().waveNumbers_ + 1.8*log10(2.0*state().maxClusterRadiusBE_*state().waveNumbers_ + PI);
	state().numExpansionTerms_ = (int)(rw);
	if(rw - (double)state().numExpansionTerms_ >= 0.5) state().numExpansionTerms_++;
	if(state().numExpansionTerms_ < state().minExpansionTermsFMM_) state().numExpansionTerms_ = state().minExpansionTermsFMM_;

	
	state().numIntegrationPointsThetaDirection_ = state().numExpansionTerms_;

	
	if(state().numIntegrationPointsThetaDirection_ > N_GAUORDER)
	  {
	    NC_Error_Exit_2(NCout,
			    "Too many integral points in the theta-direction!",
			    "Number of integration points = ", state().numIntegrationPointsThetaDirection_,
			    "Maximum allowable number of integration points = ", N_GAUORDER);
	  }

	
	state().numIntegrationPointsPhiDirection_ = 2*state().numIntegrationPointsThetaDirection_;

	
	state().numIntegrationPointsUnitSphere_ = state().numIntegrationPointsThetaDirection_*state().numIntegrationPointsPhiDirection_;
	break;
      case 3: 
	runtime->quiet << endl;
	NCout << endl;
	for(l=0; l<state().numClusterLevels_; l++)
	  { checkpoint(); 
	    
	    rd_max = state().clulevarry[l].RadiMaxLv;
	    rw = 2.0*rd_max*state().waveNumbers_ + 1.8*log10(2.0*rd_max*state().waveNumbers_ + PI);
	    terml = (int)(rw);
	    if(rw - (double)terml >= 0.5) terml++;
	    if(terml < state().minExpansionTermsFMM_) terml = state().minExpansionTermsFMM_;
	    npthet = state().clulevarry[l].nExpaTermLv = terml;

	    
	    state().clulevarry[l].nPoinThetLv = npthet;

	    
	    if(npthet > N_GAUORDER)
	      {
		NC_Error_Exit_2(NCout,
				"Too many integral points in the theta-direction!",
				"Number of integration points = ", npthet,
				"Maximum allowable number of integration points = ", N_GAUORDER);
	      }

	    
	    npphi = 2*npthet;
	    state().clulevarry[l].nPoinPhiLv = npphi;

	    
	    state().clulevarry[l].nPoinSpheLv = npthet*npphi;
	  } 

	
	state().numExpansionTerms_ = state().clulevarry[state().nlevtop_].nExpaTermLv;
	state().numIntegrationPointsThetaDirection_ = state().clulevarry[state().nlevtop_].nPoinThetLv;
	state().numIntegrationPointsPhiDirection_ = state().clulevarry[state().nlevtop_].nPoinPhiLv;
	state().numIntegrationPointsUnitSphere_ = state().clulevarry[state().nlevtop_].nPoinSpheLv;
	break;
      default:
	break;
      } 

    
    if(state().methodFMM_) {
      runtime->quiet << "\nInformations about the FMM expansion:" << endl;
      runtime->quiet <<
	"   Level    N. expa. terms    N. p. theta    N. p. phi    N. p. sphere"
	   << endl;
      NCout << "\nInformations about the FMM expansion:" << endl;
      NCout <<
	"   Level    N. expa. terms    N. p. theta    N. p. phi    N. p. sphere"
	    << endl;
      for(l=0; l<state().numClusterLevels_; l++) { checkpoint(); 
	if(state().methodFMM_ == 1) {
	  terml = state().numExpansionTerms_;
	  npthet = state().numIntegrationPointsThetaDirection_;
	  npphi = state().numIntegrationPointsPhiDirection_;
	  npsph = state().numIntegrationPointsUnitSphere_;
	} else {
	  terml = state().clulevarry[l].nExpaTermLv;
	  npthet = state().clulevarry[l].nPoinThetLv;
	  npphi = state().clulevarry[l].nPoinPhiLv;
	  npsph = state().clulevarry[l].nPoinSpheLv;
	}
	runtime->quiet << "   " << setw(3) << l << "       " << setw(5) << terml <<
	  "           " << setw(5) << npthet <<
	  "           " << setw(5) << npphi <<
	  "         " << setw(5) << npsph << endl;
	NCout << "   " << setw(3) << l << "       " << setw(5) << terml <<
	  "           " << setw(5) << npthet <<
	  "           " << setw(5) << npphi <<
	  "         " << setw(5) << npsph << endl;
      }
    }

    if( estimate_ram ) {
      
      
      uint nint = 0;
      uint ncmplx = 0;
      int i,j,nnonzers;
      Vector<bool> bneacluo(state().numOriginalClusters_);
      Vector<int> nclufar(state().numOriginalClusters_);
      int neli, nelij, nori_clu, nlv;


      ncmplx = state().numRowsOfCoefficientMatrix_; 
      switch( state().methodFMM_ ) {
      case 0: 
	runtime->quiet << "TBEM: \n";
	runtime->quiet << "Number of entries right hand side: " << ncmplx << "\n";
	runtime->quiet << "Number of Matrix entries: " << ncmplx * ncmplx << "\n";
	ncmplx += state().numRowsOfCoefficientMatrix_ * state().numRowsOfCoefficientMatrix_; 
	break;
      case 1: 
	
	ncmplx += state().numIntegrationPointsUnitSphere_ * 2;
	
	nnonzers = 0;
	for(i=0; i<state().numOriginalClusters_; i++) { checkpoint(); 
	  neli = state().ClustArray[i].NumOfDOFs;
	  bneacluo = false;
	  for(j=0; j<state().ClustArray[i].NumNeaClus; j++)
	    { checkpoint(); 
	      nori_clu = state().ClustArray[state().ClustArray[i].NumsNeaClus[j]].OriClust;
	      if(!bneacluo[nori_clu]) bneacluo[nori_clu] = true;
	    }
	  nelij = 0;
	  for(j=0; j<state().numOriginalClusters_; j++)
	    if(bneacluo[j])
	      nelij += state().ClustArray[j].NumOfDOFs;
	  nnonzers += neli*nelij;
	}

	runtime->quiet << "SLFMM:\n";
	runtime->quiet << "Nearfield: " << nnonzers << "\n";
	nint += (uint)nnonzers + (uint)state().numRowsOfCoefficientMatrix_ + 1;
	ncmplx += (uint)nnonzers;

	

	nnonzers = 0;
	for(i=0; i<state().numOriginalReflectedClusters_; i++)
	  nnonzers += state().clustarry[i].NumOfDOFs;
	nnonzers *= state().numIntegrationPointsUnitSphere_;

	ncmplx += (uint)nnonzers;
	nint += (uint)nnonzers + (uint)state().numIntegrationPointsUnitSphere_*state().numOriginalReflectedClusters_ + 1;

	runtime->quiet << "T Matrix: " << nnonzers << "\n";

	
	nnonzers = 0;
	for(i=0; i<state().numOriginalClusters_; i++)
	  { checkpoint(); 
	    nnonzers += state().clustarry[i].NumFarClus;
	  }
	nnonzers *= state().numIntegrationPointsUnitSphere_;
	nint += (uint)nnonzers + (uint)state().numOriginalClusters_*state().numIntegrationPointsUnitSphere_;
	ncmplx += (uint)nnonzers;

	runtime->quiet << "D Matrix: " << nnonzers << "\n";
	
	nnonzers = state().numRowsOfCoefficientMatrix_ * state().numIntegrationPointsUnitSphere_;
	nint += (uint)nnonzers + (uint)state().numRowsOfCoefficientMatrix_ + 1;
	ncmplx += (uint)nnonzers;

	runtime->quiet << "S Matrix: " << nnonzers << "\n";
	break;
      case 3:  
	
	ncmplx += state().numIntegrationPointsUnitSphere_ * 2;
	
	nnonzers = 0;
	for(i=0; i<state().numOriginalClusters_; i++) { checkpoint(); 
	  neli = state().ClustArray[i].NumOfDOFs;
	  bneacluo = false;
	  for(j=0; j<state().ClustArray[i].NumNeaClus; j++)
	    { checkpoint(); 
	      nori_clu = state().ClustArray[state().ClustArray[i].NumsNeaClus[j]].OriClust;
	      if(!bneacluo[nori_clu]) bneacluo[nori_clu] = true;
	    }
	  nelij = 0;
	  for(j=0; j<state().numOriginalClusters_; j++)
	    if(bneacluo[j])
	      nelij += state().ClustArray[j].NumOfDOFs;
	  nnonzers += neli*nelij;
	}

	nint += (uint)nnonzers + (uint)state().numRowsOfCoefficientMatrix_ + 1;
	ncmplx += (uint)nnonzers;

	runtime->quiet << "MLFMM: \n";
	runtime->quiet << "Nearfield " << nnonzers << "\n";

	
	for(nlv=0; nlv<state().numClusterLevels_; nlv++) { checkpoint(); 
	  
	  nnonzers = 0;
	  for(i=0; i<state().clulevarry[nlv].nClustSLv; i++)
	    nnonzers += state().clulevarry[nlv].ClastArLv[i].NumOfDOFs;
	  nnonzers *= state().clulevarry[nlv].nPoinSpheLv;

	  runtime->quiet << "Tmatrix level " << nlv << ": " << nnonzers << "\n";

	  nint += (uint)nnonzers + (uint)state().clulevarry[nlv].nClustSLv*state().clulevarry[nlv].nPoinSpheLv + 1;
	  ncmplx += (uint)nnonzers;
	}

	
	for(nlv=0; nlv<state().numClusterLevels_; nlv++) { checkpoint(); 
	  nnonzers = 0;
	  
	  if(nlv == 0) {
	    for(i=0; i<state().clulevarry[nlv].nClustOLv; i++) { checkpoint(); 
	      nclufar[i] = state().clulevarry[nlv].ClastArLv[i].NumFarClus;
            }
	  }
	  else {
            int nlf = nlv-1, ifa, jfa, jself;
            int ncluof = state().clulevarry[nlf].nClustOLv,
	      nclusf = state().clulevarry[nlf].nClustSLv;
            Matrix<bool> ifnear(ncluof, nclusf, false);
	    for(i=0; i<ncluof; i++) { checkpoint(); 
	      for(j=0; j<state().clulevarry[nlf].ClastArLv[i].NumNeaClus; j++)
		ifnear(i, state().clulevarry[nlf].ClastArLv[i].NumsNeaClus[j]) = true;
	    }
	    for(i=0; i<state().clulevarry[nlv].nClustOLv; i++) { checkpoint(); 
	      ifa = state().clulevarry[nlv].ClastArLv[i].nuFather;
	      nclufar[i] = 0;
	      for(j=0; j<state().clulevarry[nlv].ClastArLv[i].NumFarClus; j++)
                { checkpoint(); 
		  jself = state().clulevarry[nlv].ClastArLv[i].NumsFarClus[j];
		  jfa = state().clulevarry[nlv].ClastArLv[jself].nuFather;
		  if(ifnear(ifa, jfa))
		    nclufar[i]++;
                }
            }
	  } 

	  for(i=0; i<state().clulevarry[nlv].nClustOLv; i++)
	    nnonzers += nclufar[i] * state().clulevarry[nlv].nPoinSpheLv;
	  

	  nint += (uint)nnonzers + (uint)state().clulevarry[nlv].nClustOLv*state().clulevarry[nlv].nPoinSpheLv + 1;
	  ncmplx += (uint)nnonzers;

	  runtime->quiet << "D Matrix level " << nlv << ": " << nnonzers << "\n";
	} 
	
	for(nlv=0; nlv<state().numClusterLevels_; nlv++) { checkpoint(); 
	  nnonzers = state().numRowsOfCoefficientMatrix_ * state().clulevarry[nlv].nPoinSpheLv;

	  nint += (uint)nnonzers + (uint)state().numRowsOfCoefficientMatrix_ + 1;
	  ncmplx += (uint)nnonzers;

	  runtime->quiet << "S Matrix level " << nlv << ": " << nnonzers << "\n";
	}
	break;
      } 
      runtime->quiet << "Total Nr. of Integers: " << nint << "\n";
      runtime->quiet << "Total Nr. of Cmplxdble: " << ncmplx << "\n";
      

	runtime->quiet << "RAM Estimation: " << (double)(nint * sizeof(int) + ncmplx * sizeof(Complex))/1000.0/1000.0/1000.0 << " GByte\n";
	NCestim << state().currentFrequency_ + 1 << " " << state().frequency_ << " ";
	NCestim << (double)(nint * sizeof(int) + ncmplx * sizeof(Complex))/1000.0/1000.0/1000.0 << "\n";

      continue; 
    }



    

	
    for(i=0; i<state().numRowsOfCoefficientMatrix_; i++)
      { checkpoint(); 
	state().zrhs[i].set(0.0, 0.0);
      }

        
    switch(state().methodFMM_)
      {
      case 0: 
	
	state().zcoefl = allocate_array<Complex>((state().numComponentsOfCoefficientMatrix_));
	for(i=0; i<state().numComponentsOfCoefficientMatrix_; i++) state().zcoefl[i].set(0.0, 0.0);
	break;
      case 1: 
	
	NC_AllocateSDTmtxsSLFMM(NCout);

	
	state().uvcsphe = allocate_array<double*>((state().numIntegrationPointsUnitSphere_));
	for(i=0; i<state().numIntegrationPointsUnitSphere_; i++)
	  { checkpoint(); 
	    state().uvcsphe[i] = allocate_array<double>((NDIM));
	  }
	state().weisphe = allocate_array<double>((state().numIntegrationPointsUnitSphere_));

	
	if(state().boolComputeTVector_) state().ztvct = allocate_array<Complex>((state().numIntegrationPointsUnitSphere_*state().numOriginalReflectedClusters_));

	
	state().zcoefl = allocate_array<Complex>((state().irownea[state().numRowsOfCoefficientMatrix_]));
	for(i=0; i<state().irownea[state().numRowsOfCoefficientMatrix_]; i++) state().zcoefl[i].set(0.0, 0.0);

	
	state().ztmtx = allocate_array<Complex>((state().irowtmtx[state().numOriginalReflectedClusters_*state().numIntegrationPointsUnitSphere_]));

	
	state().dmtxlev[0].zDmxLv = allocate_array<Complex>((state().dmtxlev[0].nEntriesD));

	
	state().zsmtx = allocate_array<Complex>((state().irowsmtx[state().numRowsOfCoefficientMatrix_]));

	break;
      case 3: 
	
	NC_AllocateSDTmtxsMLFMM(NCout);

	
	state().zcoefl = allocate_array<Complex>((state().irownea[state().numRowsOfCoefficientMatrix_]));
	for(i=0; i<state().irownea[state().numRowsOfCoefficientMatrix_]; i++) state().zcoefl[i].set(0.0, 0.0);

	
	for(j=0; j<state().numClusterLevels_; j++)
	  { checkpoint(); 
	    
	    int nthej = state().clulevarry[j].nPoinThetLv;
	    int npsh = state().clulevarry[j].nPoinSpheLv;

	    
	    state().clulevarry[j].zwkT = allocate_array<Complex>((state().clulevarry[j].nClustSLv*npsh));
	    state().clulevarry[j].zwkS = allocate_array<Complex>((state().clulevarry[j].nClustOLv*npsh));

	    
	    state().clulevarry[j].uvcsphe = allocate_array<double*>((npsh));
	    for(i=0; i<npsh; i++) state().clulevarry[j].uvcsphe[i] = allocate_array<double>((NDIM));
	    state().clulevarry[j].weisphe = allocate_array<double>((npsh));

	    
	    state().clulevarry[j].CrdGauLv = allocate_array<double>((nthej));
	    state().clulevarry[j].WeiGauLv = allocate_array<double>((nthej));

	    
	    state().tmtxlev[j].zTmxLv = allocate_array<Complex>((state().tmtxlev[j].nEntriesT));
	    if(state().boolComputeTVector_) state().tmtxlev[j].zTvcLv = allocate_array<Complex>((state().clulevarry[j].nClustSLv*npsh));
	    state().dmtxlev[j].zDmxLv = allocate_array<Complex>((state().dmtxlev[j].nEntriesD));
	    state().smtxlev[j].zSmxLv = allocate_array<Complex>((state().smtxlev[j].nEntriesS));
	  }

	break;
      } 

    
    NC_SetupEquationSystem(NCout);

    time(&ltim[4]);
    lti_est = ltim[4] - ltim[3];
    lti_eqa += lti_est;

    
    switch(state().methodSolver_)
      {
      case 0: 
	NC_IterativeSolverCGS(NCout);
	break;
      case 4: 

	break;
      }

    
    release_array(state().zcoefl);

    switch(state().methodFMM_)
      {
      case 1:
	release_array(state().jcoltmtx);
	release_array(state().irowtmtx);

	release_array(state().jcolsmtx);
	release_array(state().irowsmtx);

	release_array(state().zsmtx);
	release_array(state().ztmtx);
	if(state().boolComputeTVector_) release_array(state().ztvct);
	break;
      case 3:
	if(state().boolComputeTVector_)
	  {
	    for(j=0; j<state().numClusterLevels_; j++) release_array(state().tmtxlev[j].zTvcLv);
	  }
	for(j=0; j<state().numClusterLevels_; j++)
	  { checkpoint(); 
	    release_array(state().tmtxlev[j].jcolTmxLv);
	    release_array(state().tmtxlev[j].irowTmxLv);
	    release_array(state().tmtxlev[j].zTmxLv);

	    release_array(state().smtxlev[j].jcolSmxLv);
	    release_array(state().smtxlev[j].irowSmxLv);
	    release_array(state().smtxlev[j].zSmxLv);
	  }
	break;
      }

    time(&ltim[5]);
    lti_sst = ltim[5] - ltim[4];
    lti_sol += lti_sst;

    
    NC_PostProcessing(NCout);

    switch(state().methodFMM_)
      {
      case 1: 
	for(i=0; i<state().numIntegrationPointsUnitSphere_; i++) release_array(state().uvcsphe[i]);
	release_array(state().uvcsphe);
	release_array(state().weisphe);

	release_array(state().dmtxlev[0].jcolDmxLv);
	release_array(state().dmtxlev[0].irowDmxLv);
	release_array(state().dmtxlev[0].zDmxLv);
	break;
      case 3: 
	for(j=0; j<state().numClusterLevels_; j++)
	  { checkpoint(); 
	    release_array(state().clulevarry[j].CrdGauLv);
	    release_array(state().clulevarry[j].WeiGauLv);

	    for(i=0; i<state().clulevarry[j].nPoinSpheLv; i++) release_array(state().clulevarry[j].uvcsphe[i]);
	    release_array(state().clulevarry[j].uvcsphe);
	    release_array(state().clulevarry[j].weisphe);

	    release_array(state().clulevarry[j].zwkT);
	    release_array(state().clulevarry[j].zwkS);


	    release_array(state().dmtxlev[j].jcolDmxLv);
	    release_array(state().dmtxlev[j].irowDmxLv);
	    release_array(state().dmtxlev[j].zDmxLv);
	  }
	break;
      }

    time(&ltim[6]);
    lti_pst = ltim[6] - ltim[5];
    lti_pos += lti_pst;

    
    NCout << "\nTime Statistic For The Current Step (In Second)\n" << endl;
    NCout << "Assembling the equation system         : " << lti_est << endl;
    NCout << "Solving the equation system            : " << lti_sst << endl;
    NCout << "Post processing                        : " << lti_pst << endl;
    NCout << "Total                                  : " <<
      lti_est + lti_sst + lti_pst << endl;

    runtime->quiet << "Assembling the equation system         : " << lti_est << endl;
    runtime->quiet << "Solving the equation system            : " << lti_sst << endl;
    runtime->quiet << "Post processing                        : " << lti_pst << endl;
    runtime->quiet << "Total                                  : " <<
      lti_est + lti_sst + lti_pst << endl;
    runtime->quiet << endl;

    
    if(state().currentFrequency_ == iend - 1)  NC_DeleteArrays(state().methodFMM_, state().numClusterLevels_, 1);

  } 

  if( !estimate_ram ) {
    NCout << "\n\n\n>> T I M E   S T A T I S T I C   (In   Second) <<\n" << endl;
    NCout << "Input                                  : " << ltim[1] - ltim[0] << endl;
    NCout << "Address computation                    : " << ltim[2] - ltim[1] << endl;
    NCout << "Assembling the equation system         : " << lti_eqa << endl;
    NCout << "Solving the equation system            : " << lti_sol << endl;
    NCout << "Post processing                        : " << lti_pos << endl;
    NCout << "The whole job                          : " << ltim[6] - ltim[0] << endl;

    runtime->quiet << "\nTime Statistic for the job (in second):" << endl;
    runtime->quiet << "Input ............................ " << ltim[1] - ltim[0] << endl;
    runtime->quiet << "Address computation .............. " << ltim[2] - ltim[1] << endl;
    runtime->quiet << "Assembling the equation system ... " << lti_eqa << endl;
    runtime->quiet << "Solving the equation system ...... " << lti_sol << endl;
    runtime->quiet << "Post processing .................. " << lti_pos << endl;
    runtime->quiet << "The whole job .................... " << ltim[6] - ltim[0] << endl;
  }

  
  release_array(state().extNumbersOfNodes);
  release_array(state().isNodeMeshOrEvalNode);
  release_array(state().extNumbersOfElements);

  for(i=0; i<state().numNodes_; i++) release_array(state().nodesCoordinates[i]);
  release_array(state().nodesCoordinates);

  for(i=0; i<state().numElements_; i++) release_array(state().elementsConnectivity[i]);
  release_array(state().elementsConnectivity);

  release_array(state().listNumberNodesPerElement);
  release_array(state().listElementProperty);
  release_array(state().listElementsElementGroup);

  release_array(state().listSymmetryPlanes);
  release_array(state().listSymmetryPlanesCoordinates);

  release_array(state().ibval);
  for(i=0; i<state().numElements_; i++) release_array(state().zbval0[i]);
  release_array(state().zbval0);
  for(i=0; i<state().numElements_; i++) release_array(state().zbvao0[i]);
  release_array(state().zbvao0);
  release_array(state().zbval1);
  release_array(state().zbval2);
  release_array(state().zbvao1);
  release_array(state().zbvao2);

  for(i=0; i<state().numElements_; i++) release_array(state().elenor[i]);
  release_array(state().elenor);
  for(i=0; i<state().numElements_; i++) release_array(state().centel[i]);
  release_array(state().centel);
  release_array(state().areael);
  for(i=0; i<state().numElements_; i++) release_array(state().jelist[i]);
  release_array(state().jelist);

  release_array(state().indexOfElementGroup);
  release_array(state().numberOfElementsInGroup);
  release_array(state().propertyOfGroup);

  release_array(state().numinw);
  release_array(state().numposo);
  for(i=0; i<state().numIncidentPlaneWaves_; i++) release_array(state().dirinw[i]);
  release_array(state().dirinw);
  for(i=0; i<nptsouc; i++) release_array(state().coorps[i]);
  release_array(state().coorps);
  for(i=0; i<state().numIncidentPlaneWaves_; i++) release_array(state().inwacurv[i]);
  release_array(state().inwacurv);
  for(i=0; i<nptsouc; i++) release_array(state().posocurv[i]);
  release_array(state().posocurv);
  release_array(state().zinwav);
  release_array(state().zposoc);
  release_array(state().zinwao);
  release_array(state().zposoo);

  for(i=0; i<state().numElements_; i++) release_array(state().ibvcurv[i]);
  release_array(state().ibvcurv);
  release_array(state().nucurv);
  release_array(state().npcurv);
  for(i=0; i<state().numCurvesFrequency_; i++) release_array(state().frqcurv[i]);
  release_array(state().frqcurv);
  for(i=0; i<state().numCurvesFrequency_; i++) release_array(state().faccurv[i]);
  release_array(state().faccurv);

  release_array(Freqs);
}


void NC_FrequencyInformations
(
	ostream& xout,
	ofstream& yout
)
{
	int i, n;
	double r_min, r_max;

	switch(state().methodFMM_)
	{
	case 0:
		xout << "\nTraditional BEM" << endl;
		break;
	case 1:
		xout << "\nSingle level fast multipole BEM" << endl;
		break;
	case 3:
		xout << "\nDirect multilevel fast multipole BEM" << endl;
		break;
	}

	
	if(state().currentFrequency_ == state().istart_)
	{
		xout << "\nNumber of equations = " << state().numRowsOfCoefficientMatrix_  << endl;
		if(state().methodFMM_ == 0) {
			xout << "Number of entries of the coefficient matrix = " << state().numComponentsOfCoefficientMatrix_ << endl;
		}
	}

	
	if(state().methodFMM_) {
		xout << "\nInformations about clusters:" << endl;
		xout << "   Level    Num. of clusters    Maximum radius   Minimum radius" << endl;
		for(i=0; i<state().numClusterLevels_; i++) { checkpoint(); 
			if(state().methodFMM_ == 1) {
				n = state().numOriginalClusters_;
				r_max = state().maxClusterRadiusBE_;
				r_min = state().minClusterRadiusBE_;
			} else {
				n = state().clulevarry[i].nClustOLv;
				r_max = state().clulevarry[i].RadiMaxLv;
				r_min = state().clulevarry[i].RadiMinLv;
			}
			xout << "   " << setw(3) << i << "       " << setw(6) << n
				<< "                " << setw(6) << r_max
				<< "         " << setw(6) << r_min << endl;
		}
		if(state().numInternalPointsClusters_) {
			xout << " resu. nets" << "  " << setw(6) << state().numInternalPointsClusters_ <<
				"                " << setw(6) << state().maxClusterRadiusRM_  <<
				"         " << setw(6) << state().minClusterRadiusRM_ << endl;
		}
	}

	switch(state().methodFMM_)
	{
	case 0:
		yout << "\nTraditional BEM" << endl;
		break;
	case 1:
		yout << "\nSingle level fast multipole BEM" << endl;
		break;
	case 3:
		yout << "\nDirect multilevel fast multipole BEM" << endl;
		break;
	}

	
	if(state().currentFrequency_ == state().istart_)
	{
		yout << "\nNumber of equations = " << state().numRowsOfCoefficientMatrix_  << endl;
		if(state().methodFMM_ == 0) {
			yout << "Number of entries of the coefficient matrix = " << state().numComponentsOfCoefficientMatrix_ << endl;
		}
	}

	
	if(state().methodFMM_) {
		yout << "\nInformations about clusters:" << endl;
		yout << "   Level    Num. of clusters    Maximum radius   Minimum radius" << endl;
		for(i=0; i<state().numClusterLevels_; i++) { checkpoint(); 
			if(state().methodFMM_ == 1) {
				n = state().numOriginalClusters_;
				r_max = state().maxClusterRadiusBE_;
				r_min = state().minClusterRadiusBE_;
			} else {
				n = state().clulevarry[i].nClustOLv;
				r_max = state().clulevarry[i].RadiMaxLv;
				r_min = state().clulevarry[i].RadiMinLv;
			}
			yout << "   " << setw(3) << i << "       " << setw(6) << n
				<< "                " << setw(6) << r_max
				<< "         " << setw(6) << r_min << endl;
		}
		if(state().numInternalPointsClusters_) {
			yout << " resu. nets" << "  " << setw(6) << state().numInternalPointsClusters_ <<
				"                " << setw(6) << state().maxClusterRadiusRM_  <<
				"         " << setw(6) << state().minClusterRadiusRM_ << endl;
		}
	}
}
