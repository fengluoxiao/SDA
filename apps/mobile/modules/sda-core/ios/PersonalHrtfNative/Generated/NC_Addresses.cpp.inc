








using namespace std;                                                                





int NC_GenerateClustersSLFMM(ofstream& NCout);
void NC_AllocateNmtx(ofstream&);
void NC_ComputeClusterArraysSLFMM(ofstream&, Vector<int>&, Vector<int>&,int&);
int NC_GenerateClustersEvalGrid(ofstream&);
void NC_GenerateClustersFMM(ofstream&, int&, Vector<int>&,Vector<int>&, int&, Vector<int>&,Vector<int>&, const int&, Vector<int>&);
void NC_ClusterReflectionsSLFMM(ofstream&);
int NC_EstimateNumLevelsMLFMM(ofstream&);
void NC_GenerateClusterTreeMLFMM(ofstream&);
int NC_GenerateClustersAtLevelMLFMM(ofstream&, const int&);
void NC_GenerateClusterArrayAtLevelMLFMM(ofstream&, Vector<int>&, Vector<int>&,int&, const int&, Vector<int>&);
void NC_ClusterReflectionsAtLevelMLFMM(ofstream&, const int&);
void NC_AllocateDmtxMLFMM(ofstream&);
void NC_AllocateTmtxMLFMM(ofstream&);
void NC_AllocateSmtxMLFMM(ofstream&);
void NC_CheckClusters(ofstream&,	const int&,	const int&,	ElCluster*);
void NC_DeleteArrays(const int&, const int&, const int&);
void NC_CancelSmallClusters(Vector<int>&, Vector<int>&,  int&,  int&);




 




int NC_DeclareArrays
(
	ofstream& NCout,
	double* Freqs,
	bool estimate_ram
)
{
	double dLamdBeL = state().waveLength_/state().diameterBE_, d_ratio[10],
		thresholrat = 0.75,  
	                        
		thresholBEM = 80.0, 

		thresholFMM1 = 1.0; 
	int Max_Dofs_TBEM = 20000, 
		Max_Dofs_FMM1 = 50000; 
	int idifcomtyp = 1, i, npair_ne_sum = 1;
	

	
	state().numClusterLevels_ = 1;

	
	if(state().methodBEM_ == 0) 
	{
		state().methodFMM_ = 0; 
	}
	else 
	{
		if(dLamdBeL > thresholBEM)
		{
			state().methodFMM_ = 0; 
		}
		else
		{
			if(state().methodBEM_ == 1) 
			{
				state().methodFMM_ = 1; 
			}
			else if(state().methodBEM_ > 1) 
			{
				if(dLamdBeL > thresholFMM1)
				{
					state().methodFMM_ = 1; 
				}
				else
				{
					state().numClusterLevels_ = NC_EstimateNumLevelsMLFMM(NCout);
					if(state().numClusterLevels_ == 1)
					{
						state().methodFMM_ = 1;
					} else {
						if(state().methodBEM_ == 4) {state().methodFMM_ = 3;}
					}
					if(state().numClusterLevels_ > 10) NC_Error_Exit_1(NCout, "state().numClusterLevels_ must <= 10!",
						"state().numClusterLevels_ = ", state().numClusterLevels_);
				}
			}
		}
	}

	
	if(state().currentFrequency_ == state().istart_) for(i=0; i<state().numClusterLevels_; i++) state().n_Pair_NeaF[i] = 1;

Labsettradibem:
	state().nlevtop_ = state().numClusterLevels_ - 1;

	if(state().currentFrequency_ > state().istart_) {
		
		if(state().methodFMM_ == state().imultipori && state().numClusterLevels_ == state().nlevmlfmori) idifcomtyp = 0;

		
		npair_ne_sum = 0;
		for(i=0; i<state().nlevmlfmori; i++) npair_ne_sum += state().n_Pair_NeaF[i];

		
		
		if(idifcomtyp == 0 && npair_ne_sum == 0) return(0);

		
		NC_DeleteArrays(state().imultipori, state().nlevmlfmori, 0);
	}

	
	for(i=0; i<state().numClusterLevels_; i++) state().n_Pair_FarD[i] = state().n_Pair_NeaF[i] = 0;
	state().numOriginalReflectedClusters_ = state().numOriginalClusters_ = 0;
	if(state().methodFMM_ == 1) 
	{
		
		state().numOriginalClusters_ = NC_GenerateClustersSLFMM(NCout);

		if(state().numOriginalClusters_ > 0) {
			NC_CheckClusters(NCout, 0, state().numOriginalClusters_, state().clustarry);

			
			if(state().numReflectionsOfElements_ > 1) NC_ClusterReflectionsSLFMM(NCout);
		} else {
			NC_Error_Exit_1(NCout, "Number of clusters must > 0!", "Number of clusters = ",
				state().numOriginalClusters_);
		}
	}
	else if(state().methodFMM_ >= 2) 
	{
		
		NC_GenerateClusterTreeMLFMM(NCout);

		state().numOriginalClusters_ = state().clulevarry[state().nlevtop_].nClustOLv;
		state().numOriginalReflectedClusters_ = state().clulevarry[state().nlevtop_].nClustSLv;
	}

	
	switch(state().methodFMM_)
	{
	case 1: 
		state().ClustArray = state().clustarry;
		break;
	case 3: 
		state().ClustArray = state().clulevarry[state().nlevtop_].ClastArLv;
		break;
	}

    
    state().numRowsOfCoefficientMatrix_ = 0;  
    for(i=0; i<state().numElements_; i++)
    { checkpoint(); 
        if(state().listElementProperty[i] == 2) continue;
        state().jelist[i][0] = state().numRowsOfCoefficientMatrix_++;
    }

    
    state().numComponentsOfCoefficientMatrix_ = state().numRowsOfCoefficientMatrix_*state().numRowsOfCoefficientMatrix_;
    
    
    
    state().numInternalPointsClusters_ = 0;
    if(state().methodFMM_) 
      {
	
	
	
	
	if( !estimate_ram )
	  NC_AllocateNmtx(NCout);
	
	
	if(state().numNodesOfEvaluationMesh_ > 0) state().numInternalPointsClusters_ = NC_GenerateClustersEvalGrid(NCout);
      }
    if ( !estimate_ram ) {
      
      switch(state().methodFMM_)
	{
	case 1:
	  state().dmtxlev = allocate_array<D_mtx_lev>((1));
	  break;
	case 3:
	  state().dmtxlev = allocate_array<D_mtx_lev>((state().numClusterLevels_));
	  state().tmtxlev = allocate_array<T_mtx_lev>((state().numClusterLevels_));
	  state().smtxlev = allocate_array<S_mtx_lev>((state().numClusterLevels_));
	  break;
	}
      
      
      state().zrhs = allocate_array<Complex>((state().numRowsOfCoefficientMatrix_));
    }
    
    state().imultipori = state().methodFMM_;
    state().nlevmlfmori = state().numClusterLevels_;
    
    
    
    for(i=0; i<state().numClusterLevels_; i++) { checkpoint(); 
      if(state().n_Pair_NeaF[i] > 0) {
	d_ratio[i] = (double)state().n_Pair_NeaF[i]/(double)state().n_Pair_FarD[i];
      } else {
	d_ratio[i] = 0.0;
	
	
      }
    }
    
    
    if(state().methodFMM_ && (state().n_Pair_NeaF[0] || state().n_Pair_NeaF[state().numClusterLevels_ - 1])) {
      runtime->quiet << "\nInformations about the canceled interacting cluster pairs:" << endl;
      runtime->quiet << "   Level   N. canceled pairs   N. interacting pairs   Cancel ratio (%)"
	   << endl;
      for(i=0; i<state().numClusterLevels_; i++) { checkpoint(); 
	runtime->quiet << "   " << setw(3) << i << "     " << setw(10) << state().n_Pair_NeaF[i] <<
	  "           " << setw(10) << state().n_Pair_FarD[i] <<
	  "               " << setw(5) << d_ratio[i]*100.0 << endl;
      }
      NCout << "\nInformations about the canceled interacting cluster pairs:" << endl;
      NCout << "   Level   N. canceled pairs   N. interacting pairs   Cancel ratio (%)"
	    << endl;
      for(i=0; i<state().numClusterLevels_; i++) { checkpoint(); 
	NCout << "   " << setw(3) << i << "     " << setw(10) << state().n_Pair_NeaF[i] <<
	  "           " << setw(10) << state().n_Pair_FarD[i] <<
	  "               " << setw(5) << d_ratio[i]*100.0 << endl;
      }
    }
    
    if(state().methodFMM_ == 1) {
      if(d_ratio[0] > thresholrat) {
	state().methodFMM_ = 0;
	state().numClusterLevels_ = 1;
	idifcomtyp = 1;
	if(state().numRowsOfCoefficientMatrix_ < Max_Dofs_TBEM) {
	  goto Labsettradibem;
	} else {
	  NC_Error_Exit_1(NCout, "Frquency is too low, the single level FMBEM can not be used! Also, the number of elements for the traditioal BEM is limited to 20000 Elements (~ 6.4Gb). Stopping NumCalc",
			  "Frequency = ", state().frequency_);
	}
      }
    } else if(state().methodFMM_ > 1) {
      for(i=0; i<state().numClusterLevels_; i++)
	{ checkpoint(); 
	  if(d_ratio[i] > thresholrat) {
	    state().methodFMM_ = 1;
	    state().numClusterLevels_ = 1;
	    idifcomtyp = 1;
	    if(state().numRowsOfCoefficientMatrix_ < Max_Dofs_FMM1) {
	      goto Labsettradibem;
	    } else {
	      NC_Error_Exit_1(NCout, "Frquency is too low, the multilevel FMBEM can not be used!",
			      "Frequency = ", state().frequency_);
	    }
	  }
	}
    }
    
    return(1);
}


int NC_GenerateClustersSLFMM
(
	ofstream& NCout
)
{
	int i, j;

	int num_clus = 0; 

	
	Vector<int> nel_clus(state().numElementsOfBoundaryMesh_), nuel_clus(state().numElementsOfBoundaryMesh_), nfath_clus(0);

	
	int nbegrp = 0;
	for(i=0; i<state().numElementGroups_; i++) if(state().propertyOfGroup[i] == 0) nbegrp++;

	
	Vector<int> nubegrp(nbegrp);
	j = 0;
	for(i=0; i<state().numElementGroups_; i++) if(state().propertyOfGroup[i] == 0) nubegrp[j++] = i;

	
	int melpergrp = 0;
	for(i=0; i<nbegrp; i++)
		if(state().numberOfElementsInGroup[nubegrp[i]] > melpergrp) melpergrp = state().numberOfElementsInGroup[nubegrp[i]];

	
	Vector<int> nuelbegrp(melpergrp);

	
	double clus_nels = sqrt((double)(state().numElementsOfBoundaryMesh_)); 

	
	if(state().ClusEdgL0_ <= 0.0) 
	{
		state().ClusEdgL1 = sqrt(clus_nels*state().averageElementArea_);
	}
	else 
	{
		state().ClusEdgL1 = state().ClusEdgL0_;
		if(state().ClusEdgL1 < sqrt(4.0*state().averageElementArea_))
		{
			NC_Error_Exit_2(NCout, "The read cluster diameter is too small!",
				"Inputed value of cluster diameter = ", state().ClusEdgL1,
				"The smallst allowable value       = ", sqrt(4.0*state().averageElementArea_));
		}
		NCout << "\nClusEdgL (Inputed value of cluster diameter) = " << state().ClusEdgL1 << endl;
		runtime->quiet << "state().ClusEdgL1 (Inputed value of cluster diameter) = " << state().ClusEdgL1 << endl;
		runtime->quiet << endl;
	}

	
	NC_GenerateClustersFMM(NCout, num_clus, nel_clus, nuel_clus, nbegrp,
				 nubegrp, nuelbegrp, 0, nfath_clus);

	
	NC_ComputeClusterArraysSLFMM(NCout, nel_clus, nuel_clus, num_clus);

	return(num_clus);
}


void NC_ComputeClusterArraysSLFMM
(
	ofstream& NCout,
	Vector<int>& nel_clus,		
	Vector<int>& nuel_clus,		
	int& num_clus				
)
{
    int i, j, k, k1, l_sum, l1, m, ie0, ind0;
    int nel_max = 0, n_nod;
    double wk0;
    double dareadis, dispo;
    Vector<double> crcent(NDIM);

    
    for(i=0; i<num_clus; i++) if(nel_clus[i] > nel_max) nel_max = nel_clus[i];

    
    Vector<int> nu_nod_cl(nel_max*NNODPE);

    
    state().numOriginalReflectedClusters_ = num_clus*state().numReflectionsOfElements_;
    state().clustarry = allocate_array<ElCluster>((state().numOriginalReflectedClusters_));

    
    l_sum = 0;
    for(i=0; i<num_clus; i++)
    { checkpoint(); 
        
        state().clustarry[i].NumOfDOFs = state().clustarry[i].NumOfEl = nel_clus[i];

        
        state().clustarry[i].NumsOfEl = allocate_array<int>((nel_clus[i]));
        for(j=0; j<nel_clus[i]; j++) state().clustarry[i].NumsOfEl[j] = nuel_clus[l_sum++];

        
        ie0 = state().clustarry[i].NumsOfEl[0];
        state().clustarry[i].NuElGr = state().listElementsElementGroup[ie0];

        
        state().clustarry[i].listElementPropertyEl = state().listElementProperty[ie0];

        
        state().clustarry[i].IfMonoEl = true;
        ind0 = state().listNumberNodesPerElement[ie0];
        for(j=1; j<nel_clus[i]; j++)
        { checkpoint(); 
            if(state().listNumberNodesPerElement[state().clustarry[i].NumsOfEl[j]] != ind0)
            {
                state().clustarry[i].IfMonoEl = false;
                break;
            }
        }

        
        if(state().ibval[ie0] == 2 || state().ibval[ie0] == 4) state().clustarry[i].IfAdmiBc = true;
        else state().clustarry[i].IfAdmiBc = false;

        
        state().clustarry[i].NDOFsPeEl = 1;

        
        n_nod = 0;
        for(j=0; j<nel_clus[i]; j++)
        { checkpoint(); 
            k1 = state().clustarry[i].NumsOfEl[j];
            for(k=0; k<state().listNumberNodesPerElement[k1]; k++)
            { checkpoint(); 
                l1 = state().elementsConnectivity[k1][k];
                for(m=0; m<n_nod; m++) if(nu_nod_cl[m] == l1) goto LbclustAray0;
                nu_nod_cl[n_nod++] = l1;
            LbclustAray0: continue;
            }
        }

        
        crcent = 0.0;
        for(j=0; j<n_nod; j++)
        { checkpoint(); 
            for(k=0; k<NDIM; k++) crcent[k] += state().nodesCoordinates[nu_nod_cl[j]][k];
        }
        for(k=0; k<NDIM; k++) state().clustarry[i].CoorCent[k] = crcent[k]/(double)n_nod;

        
        dareadis = 0.0;
        for(j=0; j<n_nod; j++)
        { checkpoint(); 
            dispo = Dispoi_dim3_(state().clustarry[i].CoorCent, state().nodesCoordinates[nu_nod_cl[j]]);
            if(dispo > dareadis) dareadis = dispo;
        }
        state().clustarry[i].RadiClus = dareadis;
    } 

    
    state().avgClusterRadiusBE_ = state().maxClusterRadiusBE_ = 0;
    state().minClusterRadiusBE_ = 1.0e40;
    for(i=0; i<num_clus; i++)
    { checkpoint(); 
        if(state().clustarry[i].RadiClus > state().maxClusterRadiusBE_) state().maxClusterRadiusBE_ = state().clustarry[i].RadiClus;
        if(state().clustarry[i].RadiClus < state().minClusterRadiusBE_) state().minClusterRadiusBE_ = state().clustarry[i].RadiClus;
        state().avgClusterRadiusBE_ += state().clustarry[i].RadiClus;
    }
    state().avgClusterRadiusBE_ /= (double)num_clus;

    
    if(state().numReflectionsOfElements_ == 1)
    {
        Matrix<bool> ifarclus(num_clus, num_clus, false);
        for(i=0; i<num_clus; i++) for(j=0; j<i; j++)
        { checkpoint(); 
            dispo = Dispoi_dim3_(state().clustarry[i].CoorCent, state().clustarry[j].CoorCent);
            wk0 = state().clustarry[i].RadiClus + state().clustarry[j].RadiClus;

            if(dispo > state().farFieldClusterFactor_*wk0) {
                if(dispo > state().minClusterDistance_) {
                    ifarclus(j, i) = ifarclus(i, j) = true;
                } else {
                    state().n_Pair_NeaF[0]++;
                }
                state().n_Pair_FarD[0]++;
            }
        }

        int nnea, nfar;
        for(i=0; i<num_clus; i++)
        { checkpoint(); 
            state().clustarry[i].NumNeaClus = state().clustarry[i].NumFarClus = 0;
            for(j=0; j<num_clus; j++)
            { checkpoint(); 
                if(ifarclus(i, j)) {state().clustarry[i].NumFarClus++;}
                else {state().clustarry[i].NumNeaClus++;}
            }

            state().clustarry[i].NumsNeaClus = allocate_array<int>((state().clustarry[i].NumNeaClus));
            state().clustarry[i].NumsFarClus = allocate_array<int>((state().clustarry[i].NumFarClus));
            nnea = nfar = 0;
            for(j=0; j<num_clus; j++)
            { checkpoint(); 
                if(ifarclus(i, j)) state().clustarry[i].NumsFarClus[nfar++] = j;
                else state().clustarry[i].NumsNeaClus[nnea++] = j;
            }
        }
    } 

    
    for(i=0; i<num_clus; i++)
    { checkpoint(); 
        state().clustarry[i].OriClust = i;
        state().clustarry[i].nuref = 0;
        state().clustarry[i].rffac = 1;
        state().clustarry[i].ifmirro = false;
        for(j=0; j<NDIM; j++) state().clustarry[i].ifrfdi[j] = false;
    }
}


void NC_ClusterReflectionsSLFMM
(
	ofstream& NCout
 )
{
    int i, j, i0 = 0, icl, nucl, kref, nrdir = 0, nel;
    double dispo, wk0;
    int nrefi[NDIM];
    int nrefdi[] = {0, 0, 1, 0, 2, 0, 1, 0};

    
    nucl = state().numOriginalClusters_;  
    for(icl=0; icl<state().numOriginalClusters_; icl++)
    { checkpoint(); 
        for(i=0; i<NDIM; i++) nrefi[i] = 0;
        for(kref=1; kref<state().numReflectionsOfElements_; kref++)
        { checkpoint(); 
            
            switch(state().numReflectionsOfElements_)
            {
                case 8:  
                    nrdir = nrefdi[kref];
                    nrefi[nrdir]++;
                    break;
                case 4:  
                    for(i=0; i<NDIM; i++) if(state().listSymmetryPlanes[i] == 0)
                    {
                        i0 = i + 1;
                        break;
                    }
                    nrdir = i0 + nrefdi[kref];
                    if(nrdir >= NDIM) nrdir -= NDIM;
                    nrefi[nrdir]++;
                    break;
                case 2:  
                    for(i=0; i<NDIM; i++) if(state().listSymmetryPlanes[i] != 0)
                    {
                        nrdir = i;
                        break;
                    }
                    nrefi[nrdir]++;
            } 

            
            state().clustarry[nucl].NuElGr = state().clustarry[icl].NuElGr;
            state().clustarry[nucl].listElementPropertyEl = state().clustarry[icl].listElementPropertyEl;
            state().clustarry[nucl].IfMonoEl = state().clustarry[icl].IfMonoEl;
            state().clustarry[nucl].IfAdmiBc = state().clustarry[icl].IfAdmiBc;
            state().clustarry[nucl].NumOfDOFs = state().clustarry[icl].NumOfDOFs;
            state().clustarry[nucl].NDOFsPeEl = state().clustarry[icl].NDOFsPeEl;

            nel = state().clustarry[nucl].NumOfEl = state().clustarry[icl].NumOfEl;
            state().clustarry[nucl].NumsOfEl = allocate_array<int>((nel));
            for(i=0; i<nel; i++) state().clustarry[nucl].NumsOfEl[i] = state().clustarry[icl].NumsOfEl[i];

            state().clustarry[nucl].RadiClus = state().clustarry[icl].RadiClus;

            for(i=0; i<NDIM; i++)
            { checkpoint(); 
                if(nrefi[i]%2) state().clustarry[nucl].ifrfdi[i] = true;
                else state().clustarry[nucl].ifrfdi[i] = false;
                if(state().clustarry[nucl].ifrfdi[i])
                    state().clustarry[nucl].CoorCent[i] = 2.0*state().listSymmetryPlanesCoordinates[i] - state().clustarry[icl].CoorCent[i];
                else
                    state().clustarry[nucl].CoorCent[i] = state().clustarry[icl].CoorCent[i];
            }

            state().clustarry[nucl].OriClust = icl;
            state().clustarry[nucl].nuref = kref;
            if(kref%2) state().clustarry[nucl].ifmirro = true;
            else  state().clustarry[nucl].ifmirro = false;

            state().clustarry[nucl].rffac = 1;
            for(i=0; i<NDIM; i++) if(state().clustarry[nucl].ifrfdi[i] && state().listSymmetryPlanes[i] < 0)
                state().clustarry[nucl].rffac *= -1;

            nucl++;
        } 
    } 

    
    Matrix<bool> ifarclus(state().numOriginalReflectedClusters_, state().numOriginalReflectedClusters_, false);
    for(i=0; i<state().numOriginalReflectedClusters_; i++) for(j=0; j<i; j++)
    { checkpoint(); 
        dispo = Dispoi_dim3_(state().clustarry[i].CoorCent, state().clustarry[j].CoorCent);
        wk0 = state().clustarry[i].RadiClus + state().clustarry[j].RadiClus;

        if(dispo > state().farFieldClusterFactor_*wk0) {
            if(dispo > state().minClusterDistance_) {
                ifarclus(j, i) = ifarclus(i, j) = true;
            } else {
                state().n_Pair_NeaF[0]++;
            }
            state().n_Pair_FarD[0]++;
        }
    }

    int nnea, nfar;
    for(i=0; i<state().numOriginalReflectedClusters_; i++)
    { checkpoint(); 
        state().clustarry[i].NumNeaClus = state().clustarry[i].NumFarClus = 0;
        for(j=0; j<state().numOriginalReflectedClusters_; j++)
        { checkpoint(); 
            if(ifarclus(i, j)) state().clustarry[i].NumFarClus++;
            else state().clustarry[i].NumNeaClus++;
        }

        state().clustarry[i].NumsNeaClus = allocate_array<int>((state().clustarry[i].NumNeaClus));
        state().clustarry[i].NumsFarClus = allocate_array<int>((state().clustarry[i].NumFarClus));
        nnea = nfar = 0;
        for(j=0; j<state().numOriginalReflectedClusters_; j++)
        { checkpoint(); 
            if(ifarclus(i, j)) state().clustarry[i].NumsFarClus[nfar++] = j;
            else state().clustarry[i].NumsNeaClus[nnea++] = j;
        }
    }
}


void NC_GenerateClustersFMM
(
	ofstream& NCout,
	int& num_clus,				
	Vector<int>& nel_clus,		
	Vector<int>& nuel_clus,		
	int& nbegr_fcl,				
	Vector<int>& nubegrp,		
	Vector<int>& nuelbegrp,		
	const int& nu_lev,			
	Vector<int>& nfath_clus		
)
{
  int i, j, k, l, ibg, nelgri, nodgri, ieli, ndij, inodi, nclus, l1, iclus, i1;
  int idexel = 0;
  Vector<int> nundgri(state().numNodesOfBoundaryMesh_), ndiv_xyz(NDIM);
  double d0, d1, dCluEdgLv = state().ClusEdgL1/pow(2.0, nu_lev);
  Vector<double> xyz_max(NDIM), xyz_min(NDIM), dif_xyz(NDIM);
  
  
  
  for(ibg=0; ibg<nbegr_fcl; ibg++) { checkpoint(); 
    
    
    nelgri = 0;
    nuelbegrp = 0;
    if(nu_lev == 0) {
      for(j=0; j<state().numElements_; j++)
	if(state().indexOfElementGroup[nubegrp[ibg]] == state().listElementsElementGroup[j])
	  nuelbegrp[nelgri++] = j;
    }
        else
        {
            nelgri = state().clulevarry[nu_lev - 1].ClastArLv[ibg].NumOfEl;
            for(j=0; j<nelgri; j++) nuelbegrp[j] =
                state().clulevarry[nu_lev - 1].ClastArLv[ibg].NumsOfEl[j];
        }

        
        nodgri = 0;
        for(i=0; i<nelgri; i++)
        { checkpoint(); 
            ieli = nuelbegrp[i];
            inodi = state().listNumberNodesPerElement[ieli];
            if(inodi > NETYP4) inodi /= 2;
            for(j=0; j<inodi; j++)
            { checkpoint(); 
                ndij = state().elementsConnectivity[ieli][j];
                for(k=0; k<nodgri; k++) if(nundgri[k] == ndij) goto Lab0clusMetho1;
                nundgri[nodgri++] = ndij;
            Lab0clusMetho1: continue;
            }
        }
	


        
        xyz_max = -1.0e20;
        xyz_min = 1.0e20;
        for(i=0; i<nodgri; i++)
        { checkpoint(); 
            ndij = nundgri[i];
            for(j=0; j<NDIM; j++)
            { checkpoint(); 
                if(state().nodesCoordinates[ndij][j] > xyz_max[j]) xyz_max[j] = state().nodesCoordinates[ndij][j];
                if(state().nodesCoordinates[ndij][j] < xyz_min[j]) xyz_min[j] = state().nodesCoordinates[ndij][j];
            }
        }

        for(j=0; j<NDIM; j++)
        { checkpoint(); 
            xyz_max[j] += state().delta_;
            xyz_min[j] -= state().delta_;
        }
        for(j=0; j<NDIM; j++) dif_xyz[j] = xyz_max[j] - xyz_min[j];

        
        if(nu_lev == 0)
        {
            for(j=0; j<NDIM; j++)
            { checkpoint(); 
                d0 = dif_xyz[j]/state().ClusEdgL1;
                i = (int)(d0);
                if(i == 0) i = 1; else if(d0 - (double)(i) > 0.5) i++;
                ndiv_xyz[j] = i;
                dif_xyz[j] /= (double)(i);
            }
        }
        else
        {
            for(j=0; j<NDIM; j++)
            { checkpoint(); 
                d0 = dif_xyz[j]/dCluEdgLv;
                i = (int)(d0);
                if(i == 0) i = 1;
                else
                {
                    if(d0 - (double)(i) > 0.5) i++;
                    if(i > 2) i = 2;
                }
                ndiv_xyz[j] = i;
                dif_xyz[j] /= (double)(i);
            }
        }

        
        nclus = ndiv_xyz[0]*ndiv_xyz[1]*ndiv_xyz[2];

        
        Vector<int> ne_clus(nclus, 0), nue_clus(nelgri);

        
        Matrix<int> nuindxyz(nelgri, NDIM, -1);
        for(j=0; j<NDIM; j++)
        { checkpoint(); 
            d1 = xyz_min[j];
            for(k=0; k<ndiv_xyz[j]; k++)
            { checkpoint(); 
	      d0 = d1; 
	      d1 += dif_xyz[j];
	      
                for(i=0; i<nelgri; i++)
                { checkpoint(); 
                    ieli = nuelbegrp[i];
                    if(nuindxyz(i, j) == -1 && state().centel[ieli][j] > d0 - state().epsilon_ &&
                       state().centel[ieli][j] < d1 + state().epsilon_) nuindxyz(i, j) = k;
                }
            }
        }

        
        for(i=0; i<nelgri; i++) for(j=0; j<NDIM; j++) { checkpoint(); 
            if(nuindxyz(i, j) == -1) {
                NC_Error_Exit_2(NCout, "The interval number of an element is not found!",
                                "number of the element = ", state().extNumbersOfElements[i],
                                "number of the direction = ", j);
            }
        }

        
        
        iclus = 0;
        i1 = 0;
        for(i=0; i<ndiv_xyz[0]; i++) for(j=0; j<ndiv_xyz[1]; j++)
            for(k=0; k<ndiv_xyz[2]; k++)
            { checkpoint(); 
                l1 = 0;
                for(l=0; l<nelgri; l++)
                    if(nuindxyz(l, 0) == i && nuindxyz(l, 1) == j && nuindxyz(l, 2) == k)
                    {
                        l1++;
                        nue_clus[i1++] = nuelbegrp[l];
                    }
                ne_clus[iclus++] = l1;
            }
        
        
        NC_CancelSmallClusters(ne_clus, nue_clus, nclus, ibg);



        
        i1 = 0;
        for(i=0; i<nclus; i++)
        { checkpoint(); 
            if(ne_clus[i] == 0) continue;

            if(nu_lev > 0) nfath_clus[num_clus] = ibg;
            nel_clus[num_clus++] = ne_clus[i];
            for(j=0; j<ne_clus[i]; j++) nuel_clus[idexel++] = nue_clus[i1++];
        }

    } 
}



int NC_EstimateNumLevelsMLFMM
(
	ofstream& NCout
)
{
	int n_levs;
	double ele_leng = sqrt(state().averageElementArea_), d_levs,
		d_min = 25.0; 
	double nclusroot, 
		clu_l_min;    

	
    nclusroot = sqrt((double)state().numElementsOfBoundaryMesh_)*0.9;
    if(nclusroot < d_min) nclusroot = d_min;
    clu_l_min = sqrt(d_min)*ele_leng;

	
	if(state().ClusEdgL0_ <= 0.0) 
	{
		double clus_nels = (double)state().numElementsOfBoundaryMesh_/nclusroot;
		state().ClusEdgL1 = sqrt(clus_nels)*ele_leng;
	}
	else 
	{
		state().ClusEdgL1 = state().ClusEdgL0_;
		if(state().ClusEdgL1 < clu_l_min)
		{
			NC_Error_Exit_2(NCout,
				"Inputed value of cluster diameter for level 0 is too small!",
				"Inputed value of cluster diameter for level 0 = ", state().ClusEdgL1,
				"Smallst allowable value of cluster diameter for level 0 = ", clu_l_min);
		}
	}

	d_levs = log10(state().ClusEdgL1/clu_l_min)/log10(2.0);
	n_levs = (int)d_levs;
	if(d_levs - (double)n_levs > 0.5) n_levs++;
	n_levs++;
	if(n_levs <= 0) n_levs = 1;

	return(n_levs);
}


void NC_GenerateClusterTreeMLFMM
(
	ofstream& NCout
)
{
	int i, ncl;

	
	state().clulevarry = allocate_array<ClusterLev>((state().numClusterLevels_));

	
	for(i=0; i<state().numClusterLevels_; i++)
	{ checkpoint(); 
		ncl = NC_GenerateClustersAtLevelMLFMM(NCout, i);
		NC_CheckClusters(NCout, i, state().clulevarry[i].nClustOLv, state().clulevarry[i].ClastArLv);
	}
}


int NC_GenerateClustersAtLevelMLFMM
(
	ofstream& NCout,
	const int& nu_lev	
)
{
	int i, j;
	int nbegrp_facl, melpergrp=0;
	int num_clus = 0; 

	
	Vector<int> nel_clus(state().numElementsOfBoundaryMesh_), nuel_clus(state().numElementsOfBoundaryMesh_), nfath_clus(state().numElementsOfBoundaryMesh_);

	if(nu_lev == 0) 
	{
		
		int nbegrp_facl = 0;
		for(i=0; i<state().numElementGroups_; i++)
		{ checkpoint(); 
			if(state().propertyOfGroup[i] == 0) nbegrp_facl++;
		}

		
		Vector<int> nubegrp(nbegrp_facl);
		j = 0;
		for(i=0; i<state().numElementGroups_; i++)
		{ checkpoint(); 
			if(state().propertyOfGroup[i] == 0) nubegrp[j++] = i;
		}

		
		for(i=0; i<nbegrp_facl; i++)
			if(state().numberOfElementsInGroup[nubegrp[i]] > melpergrp) melpergrp = state().numberOfElementsInGroup[nubegrp[i]];
		Vector<int> nuelbegrp(melpergrp);

		
		NC_GenerateClustersFMM(NCout, num_clus, nel_clus, nuel_clus, nbegrp_facl,
					 nubegrp, nuelbegrp, nu_lev, nfath_clus);
	}
	else 
	{
		
		nbegrp_facl = state().clulevarry[nu_lev - 1].nClustOLv;

		
		Vector<int> nubegrp(0);

		
		for(i=0; i<nbegrp_facl; i++)
		{ checkpoint(); 
			j = state().clulevarry[nu_lev - 1].ClastArLv[i].NumOfEl;
			if(j > melpergrp) melpergrp = j;
		}
		Vector<int> nuelbegrp(melpergrp);

		
		NC_GenerateClustersFMM(NCout, num_clus, nel_clus, nuel_clus, nbegrp_facl,
					 nubegrp, nuelbegrp, nu_lev, nfath_clus);
	} 

	
	NC_GenerateClusterArrayAtLevelMLFMM(NCout, nel_clus, nuel_clus, num_clus, nu_lev, nfath_clus);

	
	if(state().numReflectionsOfElements_ > 1) NC_ClusterReflectionsAtLevelMLFMM(NCout, nu_lev);

	return(num_clus);
}


void NC_GenerateClusterArrayAtLevelMLFMM
(
	ofstream& NCout,
	Vector<int>& nel_clus,		
	Vector<int>& nuel_clus,		
	int& num_clus,				
	const int& nu_lev,			
	Vector<int>& nfath_clus		
)
{
	int i, j, k, k1, l_sum, l1, m, ie0, ind0;
	int nel_max = 0, n_nod;
	double wk0;
	double dareadis, dispo;
	Vector<double> crcent(NDIM);

	
	for(i=0; i<num_clus; i++) if(nel_clus[i] > nel_max) nel_max = nel_clus[i];

	
	Vector<int> nu_nod_cl(nel_max*4);

	
	state().clulevarry[nu_lev].nClustOLv = num_clus;
	state().clulevarry[nu_lev].nClustSLv = num_clus*state().numReflectionsOfElements_;

	
	state().clulevarry[nu_lev].ClastArLv = allocate_array<ElCluster>((num_clus*state().numReflectionsOfElements_));

	
	l_sum = 0;

	for(i=0; i<num_clus; i++)
	{ checkpoint(); 
		
		state().clulevarry[nu_lev].ClastArLv[i].NumOfDOFs =
			state().clulevarry[nu_lev].ClastArLv[i].NumOfEl = nel_clus[i];

		
		state().clulevarry[nu_lev].ClastArLv[i].NumsOfEl = allocate_array<int>((nel_clus[i]));
		for(j=0; j<nel_clus[i]; j++) { checkpoint(); 
		  
		  state().clulevarry[nu_lev].ClastArLv[i].NumsOfEl[j] = nuel_clus[l_sum++];
		}

		
		ie0 = state().clulevarry[nu_lev].ClastArLv[i].NumsOfEl[0];
		state().clulevarry[nu_lev].ClastArLv[i].NuElGr = state().listElementsElementGroup[ie0];

		
		state().clulevarry[nu_lev].ClastArLv[i].listElementPropertyEl = state().listElementProperty[ie0];

		
		state().clulevarry[nu_lev].ClastArLv[i].IfMonoEl = true;
		ind0 = state().listNumberNodesPerElement[ie0];
		for(j=1; j<nel_clus[i]; j++)
		{ checkpoint(); 
			if(state().listNumberNodesPerElement[state().clulevarry[nu_lev].ClastArLv[i].NumsOfEl[j]] != ind0)
			{
				state().clulevarry[nu_lev].ClastArLv[i].IfMonoEl = false;
				break;
			}
		}

		
		if(state().ibval[ie0] == 2 || state().ibval[ie0] == 4)
			state().clulevarry[nu_lev].ClastArLv[i].IfAdmiBc = true;
		else state().clulevarry[nu_lev].ClastArLv[i].IfAdmiBc = false;

		
        state().clulevarry[nu_lev].ClastArLv[i].NDOFsPeEl = 1;

		
		n_nod = 0;
		for(j=0; j<nel_clus[i]; j++)
		{ checkpoint(); 
			k1 = state().clulevarry[nu_lev].ClastArLv[i].NumsOfEl[j];
			for(k=0; k<state().listNumberNodesPerElement[k1]; k++)
			{ checkpoint(); 
				l1 = state().elementsConnectivity[k1][k];
				for(m=0; m<n_nod; m++) if(nu_nod_cl[m] == l1) goto LbclustAray2;
				nu_nod_cl[n_nod++] = l1;
LbclustAray2: continue;
			}
		}

		
		crcent = 0.0;
		for(j=0; j<n_nod; j++)
		{ checkpoint(); 
			for(k=0; k<NDIM; k++) crcent[k] += state().nodesCoordinates[nu_nod_cl[j]][k];
		}
		for(k=0; k<NDIM; k++)
			state().clulevarry[nu_lev].ClastArLv[i].CoorCent[k] = crcent[k]/(double)n_nod;

		
		dareadis = 0.0;
		for(j=0; j<n_nod; j++)
		{ checkpoint(); 
			dispo = Dispoi_dim3_(state().clulevarry[nu_lev].ClastArLv[i].CoorCent,
				state().nodesCoordinates[nu_nod_cl[j]]);
			if(dispo > dareadis) dareadis = dispo;
		}
		state().clulevarry[nu_lev].ClastArLv[i].RadiClus = dareadis;

	} 

	
	state().avgClusterRadiusBE_ = state().maxClusterRadiusBE_ = 0;
	state().minClusterRadiusBE_ = 1.0e40;
	for(i=0; i<num_clus; i++)
	{ checkpoint(); 
		if(state().clulevarry[nu_lev].ClastArLv[i].RadiClus > state().maxClusterRadiusBE_)
			state().maxClusterRadiusBE_ = state().clulevarry[nu_lev].ClastArLv[i].RadiClus;
		if(state().clulevarry[nu_lev].ClastArLv[i].RadiClus < state().minClusterRadiusBE_)
			state().minClusterRadiusBE_ = state().clulevarry[nu_lev].ClastArLv[i].RadiClus;
		state().avgClusterRadiusBE_ += state().clulevarry[nu_lev].ClastArLv[i].RadiClus;
	}
	state().avgClusterRadiusBE_ /= (double)num_clus;
	state().clulevarry[nu_lev].RadiMaxLv = state().maxClusterRadiusBE_;
	state().clulevarry[nu_lev].RadiAveLv = state().avgClusterRadiusBE_;
	state().clulevarry[nu_lev].RadiMinLv = state().minClusterRadiusBE_;

	
	if(state().numReflectionsOfElements_ == 1)
	{
		Matrix<bool> ifarclus(num_clus, num_clus, false);
		for(i=0; i<num_clus; i++) for(j=0; j<i; j++)
		{ checkpoint(); 
			dispo = Dispoi_dim3_(state().clulevarry[nu_lev].ClastArLv[i].CoorCent,
				state().clulevarry[nu_lev].ClastArLv[j].CoorCent);

			wk0 = state().clulevarry[nu_lev].ClastArLv[i].RadiClus +
				state().clulevarry[nu_lev].ClastArLv[j].RadiClus;

			if(dispo > state().farFieldClusterFactor_*wk0) {
				if(dispo > state().minClusterDistance_) {
					ifarclus(j, i) = ifarclus(i, j) = true;
				} else {
					state().n_Pair_NeaF[nu_lev]++;
				}
				state().n_Pair_FarD[nu_lev]++;
			}
		}

		int nnea, nfar;
		for(i=0; i<num_clus; i++)
		{ checkpoint(); 
			state().clulevarry[nu_lev].ClastArLv[i].NumNeaClus =
				state().clulevarry[nu_lev].ClastArLv[i].NumFarClus = 0;
			for(j=0; j<num_clus; j++)
				if(ifarclus(i, j)) state().clulevarry[nu_lev].ClastArLv[i].NumFarClus++;
				else state().clulevarry[nu_lev].ClastArLv[i].NumNeaClus++;

			state().clulevarry[nu_lev].ClastArLv[i].NumsNeaClus =
				allocate_array<int>((state().clulevarry[nu_lev].ClastArLv[i].NumNeaClus));
			state().clulevarry[nu_lev].ClastArLv[i].NumsFarClus =
				allocate_array<int>((state().clulevarry[nu_lev].ClastArLv[i].NumFarClus));
			nnea = nfar = 0;
			for(j=0; j<num_clus; j++) if(ifarclus(i, j))
				state().clulevarry[nu_lev].ClastArLv[i].NumsFarClus[nfar++] = j;
			else state().clulevarry[nu_lev].ClastArLv[i].NumsNeaClus[nnea++] = j;
		}
	} 

	
	for(i=0; i<num_clus; i++)
	{ checkpoint(); 
		state().clulevarry[nu_lev].ClastArLv[i].OriClust = i;
		state().clulevarry[nu_lev].ClastArLv[i].nuref = 0;
		state().clulevarry[nu_lev].ClastArLv[i].rffac = 1;
		state().clulevarry[nu_lev].ClastArLv[i].ifmirro = false;
		for(j=0; j<NDIM; j++) state().clulevarry[nu_lev].ClastArLv[i].ifrfdi[j] = false;
	}

	
	if(nu_lev > 0)
	{
		int nfacl, nl_f = nu_lev - 1;

		for(i=0; i<state().clulevarry[nl_f].nClustOLv; i++)
			state().clulevarry[nl_f].ClastArLv[i].n_Son = 0;

		for(i=0; i<num_clus; i++)
		{ checkpoint(); 
			nfacl = nfath_clus[i];
			state().clulevarry[nu_lev].ClastArLv[i].nuFather = nfacl;
			state().clulevarry[nl_f].ClastArLv[nfacl].nuSon
				[state().clulevarry[nl_f].ClastArLv[nfacl].n_Son++] = i;
		}
	}

	
	for(i=0; i<num_clus; i++) state().clulevarry[nu_lev].ClastArLv[i].nuLev = nu_lev;
}


int NC_GenerateClustersEvalGrid
(
	ofstream& NCout
 )
{
    int i, j, k, l, ind, iclus, nclus, l1, i1;
    double d0, d1, radi = 0.0;
    Vector<double> xyz_max(NDIM, -1.0e20), xyz_min(NDIM, 1.0e20), dif_xyz(NDIM);
    Vector<int> nuinnode(state().numNodesOfEvaluationMesh_), ndiv_xyz(NDIM);
    Matrix<int> nuindxyz(state().numNodesOfEvaluationMesh_, NDIM, -1);

    
    j = 0;
    for(i=0; i<state().numNodes_; i++) if(state().isNodeMeshOrEvalNode[i] == -1) nuinnode[j++] = i;

    
    for(i=0; i<state().numNodesOfEvaluationMesh_; i++)
    { checkpoint(); 
        ind = nuinnode[i];
        for(j=0; j<NDIM; j++)
        { checkpoint(); 
            if(state().nodesCoordinates[ind][j] > xyz_max[j]) xyz_max[j] = state().nodesCoordinates[ind][j];
            if(state().nodesCoordinates[ind][j] < xyz_min[j]) xyz_min[j] = state().nodesCoordinates[ind][j];
        }
    }
    for(j=0; j<NDIM; j++)
    { checkpoint(); 
        xyz_max[j] += state().delta_;
        xyz_min[j] -= state().delta_;
    }
    for(j=0; j<NDIM; j++) dif_xyz[j] = xyz_max[j] - xyz_min[j];

    
    for(j=0; j<NDIM; j++)
    { checkpoint(); 
        d0 = dif_xyz[j]/(state().ClusEdgL1);
        i = (int)(d0);
        if(i == 0) i = 1;
        else if(d0 - (double)(i) > 0.5) i++;
        ndiv_xyz[j] = i;
        dif_xyz[j] /= (double)(i);
    }


    nclus = ndiv_xyz[0]*ndiv_xyz[1]*ndiv_xyz[2];
    if( (nclus > state().numNodesOfEvaluationMesh_) ) {
      
      
      iclus = state().numNodesOfEvaluationMesh_;
      if(iclus > 0) state().ipcluarry = allocate_array<IpCluster>((iclus));
      for (i = 0; i < iclus; i++) { checkpoint(); 
	state().ipcluarry[i].NumOfIps = 1;
	state().ipcluarry[i].NumsOfIps = allocate_array<int>((1));
	state().ipcluarry[i].NumsOfIps[0] = i;
	ind = nuinnode[i];
	for (j = 0; j < NDIM; j++) { checkpoint(); 
	  state().ipcluarry[i].CoorCent[j] = state().nodesCoordinates[ind][j];
	}
	state().ipcluarry[i].RadiClus = 0.0;
      }
    }
    else {
      
      
      for(j=0; j<NDIM; j++)
	{ checkpoint(); 
	  d0 = xyz_min[j];
	  for(k=0; k<ndiv_xyz[j]; k++)
	    { checkpoint(); 
	      d1 = d0 + dif_xyz[j];
	      for(i=0; i<state().numNodesOfEvaluationMesh_; i++)
		{ checkpoint(); 
		  ind = nuinnode[i];
		  if(nuindxyz(i, j) == -1 && state().nodesCoordinates[ind][j] >= d0 && state().nodesCoordinates[ind][j] < d1)
                    nuindxyz(i, j) = k;
		}
	      d0 = d1;
	    }
	}
      
      
      for(i=0; i<state().numNodesOfEvaluationMesh_; i++) for(j=0; j<NDIM; j++){ checkpoint(); 
	  if(nuindxyz(i, j) == -1) {
            NC_Error_Exit_2(NCout, "The interval number of a internal point is not found!",
                            "number of the internal point = ", state().extNumbersOfNodes[nuinnode[i]],
                            "number of the direction = ", j);
	  }
	}
      
      
      
      
      Vector<int> nip_clus(nclus, 0), nuip_clus(state().numNodesOfEvaluationMesh_);
      
      iclus = 0;
      i1 = 0;
      for(i=0; i<ndiv_xyz[0]; i++) for(j=0; j<ndiv_xyz[1]; j++) for(k=0; k<ndiv_xyz[2]; k++)
								  { checkpoint(); 
								    l1 = 0;
								    for(l=0; l<state().numNodesOfEvaluationMesh_; l++)
								      if(nuindxyz(l, 0) == i && nuindxyz(l, 1) == j && nuindxyz(l, 2) == k)
									{
									  l1++;
									  nuip_clus[i1++] = l;
									}
								    nip_clus[iclus] = l1;
								    
								    iclus++;
								  }
      
      
      iclus = 0;
      for(j=0; j<nclus; j++) if(nip_clus[j] > 0) iclus++;
      
      
      if(iclus > 0) state().ipcluarry = allocate_array<IpCluster>((iclus));
      
      l1 = l = 0;
      state().minClusterRadiusRM_ = 1.0e15;
      state().avgClusterRadiusRM_ = state().maxClusterRadiusRM_ = 0.0;
      for(i=0; i<nclus; i++)
	{ checkpoint(); 
	  
	  if(nip_clus[i] == 0) continue;
	  
	  
	  state().ipcluarry[l1].NumOfIps = nip_clus[i];
	  
	  
	  state().ipcluarry[l1].NumsOfIps = allocate_array<int>((nip_clus[i]));
	  for(j=0; j<nip_clus[i]; j++) state().ipcluarry[l1].NumsOfIps[j] = nuip_clus[l++];
	  
	  
	  for(j=0; j<NDIM; j++) xyz_max[j] = 0.0;
	  for(j=0; j<state().ipcluarry[l1].NumOfIps; j++)
	    { checkpoint(); 
	      ind = nuinnode[state().ipcluarry[l1].NumsOfIps[j]];
	      for(k=0; k<NDIM; k++) xyz_max[k] += state().nodesCoordinates[ind][k];
	    }
	  for(j=0; j<NDIM; j++) state().ipcluarry[l1].CoorCent[j] =
				  xyz_max[j]/(double)(state().ipcluarry[l1].NumOfIps);
	  
	  
	  radi = 0.0;
	  for(j=0; j<state().ipcluarry[l1].NumOfIps; j++)
	    { checkpoint(); 
	      ind = nuinnode[state().ipcluarry[l1].NumsOfIps[j]];
	      d0 = 0.0;
	      for(k=0; k<NDIM; k++)
		{ checkpoint(); 
		  d1 = state().nodesCoordinates[ind][k] - state().ipcluarry[l1].CoorCent[k];
		  d0 += d1*d1;
		}
	      d0 = sqrt(d0);
	      if(d0 > radi) radi = d0;
	    }
	  state().ipcluarry[l1].RadiClus = radi;
	  
	  
	  if(radi > state().maxClusterRadiusRM_) state().maxClusterRadiusRM_ = radi;
	  if(radi < state().minClusterRadiusRM_) state().minClusterRadiusRM_ = radi;
	  state().avgClusterRadiusRM_ += radi;
	  l1++;
	}
      state().avgClusterRadiusRM_ /= (double)l1;
      
      if(state().methodFMM_ >= 2) {radi = state().clulevarry[0].RadiMaxLv;}
      else if(state().methodFMM_ == 1) {radi = state().maxClusterRadiusBE_;}
      if(state().maxClusterRadiusRM_ > radi)
	{
	  NC_Error_Warning_2(NCout, "Maximum radius of evaluation clusters >  that of BE clusters!",
			     "Maximum radius of evaluation clusters = ", state().maxClusterRadiusRM_,
			     "Maximum radius of BE clusters = ", radi);
	}
    } 
    return(iclus);
}


void NC_ClusterReflectionsAtLevelMLFMM
(
	ofstream& NCout,
	const int& nu_lev			
)
{
	int i, j, i0 = 0, icl, nucl, kref, nrdir = 0, nel;
	double dispo, wk0;
	int nrefi[NDIM];
	int nrefdi[] = {0, 0, 1, 0, 2, 0, 1, 0};

	
	nucl = state().clulevarry[nu_lev].nClustOLv; 
	for(icl=0; icl<state().clulevarry[nu_lev].nClustOLv; icl++)
	{ checkpoint(); 
		for(i=0; i<NDIM; i++) nrefi[i] = 0;
		for(kref=1; kref<state().numReflectionsOfElements_; kref++)
		{ checkpoint(); 
			
			switch(state().numReflectionsOfElements_)
			{
			case 8:  
				nrdir = nrefdi[kref];
				nrefi[nrdir]++;
				break;
			case 4:  
				for(i=0; i<NDIM; i++) if(state().listSymmetryPlanes[i] == 0)
				{
					i0 = i + 1;
					break;
				}
				nrdir = i0 + nrefdi[kref];
				if(nrdir >= NDIM) nrdir -= NDIM;
				nrefi[nrdir]++;
				break;
			case 2:  
				for(i=0; i<NDIM; i++) if(state().listSymmetryPlanes[i] != 0)
				{
					nrdir = i;
					break;
				}
				nrefi[nrdir]++;
			} 

			
			state().clulevarry[nu_lev].ClastArLv[nucl].NuElGr = state().clulevarry[nu_lev].ClastArLv[icl].NuElGr;
			state().clulevarry[nu_lev].ClastArLv[nucl].listElementPropertyEl = state().clulevarry[nu_lev].ClastArLv[icl].listElementPropertyEl;
			state().clulevarry[nu_lev].ClastArLv[nucl].IfMonoEl = state().clulevarry[nu_lev].ClastArLv[icl].IfMonoEl;
			state().clulevarry[nu_lev].ClastArLv[nucl].IfAdmiBc = state().clulevarry[nu_lev].ClastArLv[icl].IfAdmiBc;
			state().clulevarry[nu_lev].ClastArLv[nucl].NumOfDOFs = state().clulevarry[nu_lev].ClastArLv[icl].NumOfDOFs;
			state().clulevarry[nu_lev].ClastArLv[nucl].NDOFsPeEl = state().clulevarry[nu_lev].ClastArLv[icl].NDOFsPeEl;

			nel = state().clulevarry[nu_lev].ClastArLv[nucl].NumOfEl =
				state().clulevarry[nu_lev].ClastArLv[icl].NumOfEl;
			state().clulevarry[nu_lev].ClastArLv[nucl].NumsOfEl = allocate_array<int>((nel));
			for(i=0; i<nel; i++) state().clulevarry[nu_lev].ClastArLv[nucl].NumsOfEl[i] =
				state().clulevarry[nu_lev].ClastArLv[icl].NumsOfEl[i];

			state().clulevarry[nu_lev].ClastArLv[nucl].RadiClus = state().clulevarry[nu_lev].ClastArLv[icl].RadiClus;

			for(i=0; i<NDIM; i++)
			{ checkpoint(); 
				if(nrefi[i]%2) state().clulevarry[nu_lev].ClastArLv[nucl].ifrfdi[i] = true;
				else state().clulevarry[nu_lev].ClastArLv[nucl].ifrfdi[i] = false;
				if(state().clulevarry[nu_lev].ClastArLv[nucl].ifrfdi[i])
					state().clulevarry[nu_lev].ClastArLv[nucl].CoorCent[i] = 2.0*state().listSymmetryPlanesCoordinates[i] -
						state().clulevarry[nu_lev].ClastArLv[icl].CoorCent[i];
				else
					state().clulevarry[nu_lev].ClastArLv[nucl].CoorCent[i] =
						state().clulevarry[nu_lev].ClastArLv[icl].CoorCent[i];
			}

			state().clulevarry[nu_lev].ClastArLv[nucl].OriClust = icl;
			state().clulevarry[nu_lev].ClastArLv[nucl].nuref = kref;
			if(kref%2) state().clulevarry[nu_lev].ClastArLv[nucl].ifmirro = true;
			else state().clulevarry[nu_lev].ClastArLv[nucl].ifmirro = false;

			state().clulevarry[nu_lev].ClastArLv[nucl].rffac = 1;
			for(i=0; i<NDIM; i++) { checkpoint(); 
				if(state().clulevarry[nu_lev].ClastArLv[nucl].ifrfdi[i] && state().listSymmetryPlanes[i] < 0)
					state().clulevarry[nu_lev].ClastArLv[nucl].rffac *= -1;
			}

			nucl++;
		} 
	} 

	
	int nclusLev = state().clulevarry[nu_lev].nClustSLv;
	Matrix<bool> ifarclus(nclusLev, nclusLev, false);

	for(i=0; i<nclusLev; i++) for(j=0; j<i; j++)
	{ checkpoint(); 
		dispo = Dispoi_dim3_(state().clulevarry[nu_lev].ClastArLv[i].CoorCent,
			state().clulevarry[nu_lev].ClastArLv[j].CoorCent);
		wk0 = state().clulevarry[nu_lev].ClastArLv[i].RadiClus +
			state().clulevarry[nu_lev].ClastArLv[j].RadiClus;

		if(dispo > state().farFieldClusterFactor_*wk0) {
			if(dispo > state().minClusterDistance_) {
				ifarclus(j, i) = ifarclus(i, j) = true;
			} else {
				state().n_Pair_NeaF[nu_lev]++;
			}
			state().n_Pair_FarD[nu_lev]++;
		}
	}

	int nnea, nfar;
	for(i=0; i<nclusLev; i++)
	{ checkpoint(); 
		state().clulevarry[nu_lev].ClastArLv[i].NumNeaClus =
			state().clulevarry[nu_lev].ClastArLv[i].NumFarClus = 0;
		for(j=0; j<nclusLev; j++)
			if(ifarclus(i, j)) state().clulevarry[nu_lev].ClastArLv[i].NumFarClus++;
			else state().clulevarry[nu_lev].ClastArLv[i].NumNeaClus++;

		state().clulevarry[nu_lev].ClastArLv[i].NumsNeaClus = allocate_array<int>((state().clulevarry[nu_lev].ClastArLv[i].NumNeaClus));
		state().clulevarry[nu_lev].ClastArLv[i].NumsFarClus = allocate_array<int>((state().clulevarry[nu_lev].ClastArLv[i].NumFarClus));
		nnea = nfar = 0;
		for(j=0; j<nclusLev; j++)
			if(ifarclus(i, j)) state().clulevarry[nu_lev].ClastArLv[i].NumsFarClus[nfar++] = j;
			else state().clulevarry[nu_lev].ClastArLv[i].NumsNeaClus[nnea++] = j;
	}


	
	if(nu_lev > 0)
	{
		int nfacl, nrefres = state().numReflectionsOfElements_ - 1, nclofa = state().clulevarry[nu_lev - 1].nClustOLv,
			nl_f = nu_lev - 1;

		nucl = state().clulevarry[nu_lev].nClustOLv;
		for(icl=0; icl<state().clulevarry[nu_lev].nClustOLv; icl++)
		{ checkpoint(); 
			nfacl = nclofa + nrefres*state().clulevarry[nu_lev].ClastArLv[icl].nuFather;
			for(kref=1; kref<state().numReflectionsOfElements_; kref++)
			{ checkpoint(); 
				state().clulevarry[nu_lev].ClastArLv[nucl++].nuFather = nfacl++;
			}
		}

		for(i=state().clulevarry[nl_f].nClustOLv; i<state().clulevarry[nl_f].nClustSLv; i++)
			state().clulevarry[nl_f].ClastArLv[i].n_Son = 0;

		for(i=state().clulevarry[nu_lev].nClustOLv; i<state().clulevarry[nu_lev].nClustSLv; i++)
		{ checkpoint(); 
			nfacl = state().clulevarry[nu_lev].ClastArLv[i].nuFather;
			state().clulevarry[nl_f].ClastArLv[nfacl].nuSon
				[state().clulevarry[nl_f].ClastArLv[nfacl].n_Son++] = i;
		}
	}

	
	for(i=state().clulevarry[nu_lev].nClustOLv; i<state().clulevarry[nu_lev].nClustSLv; i++)
		state().clulevarry[nu_lev].ClastArLv[i].nuLev = nu_lev;
}


void NC_AllocateSDTmtxsSLFMM
(
	ofstream& NCout
 )
{
    int i, j, k;
    int nnonzerdmtx, nnonzertmtx, nnonzersmtx;
    int inoze, nnoze, k1, idfs;
    int nnze, irowd, nrowdmtx;
    int ij, kcol, itr;

    
    nnonzertmtx = 0; 
    for(i=0; i<state().numOriginalReflectedClusters_; i++) nnonzertmtx += state().clustarry[i].NumOfDOFs;
    nnonzertmtx = index_product(nnonzertmtx,state().numIntegrationPointsUnitSphere_);

    
    
    state().jcoltmtx = allocate_array<int>((nnonzertmtx/state().numIntegrationPointsUnitSphere_));
    state().irowtmtx = allocate_array<int>((state().numIntegrationPointsUnitSphere_*state().numOriginalReflectedClusters_ + 1));

    inoze = nnoze = 0;
    for(i=0; i<state().numOriginalReflectedClusters_; i++)
    { checkpoint(); 
        idfs = state().clustarry[i].NDOFsPeEl;
        for(j=0; j<state().numIntegrationPointsUnitSphere_; j++)
        { checkpoint(); 
            state().irowtmtx[inoze++] = nnoze;
            if (j != 0) { nnoze += state().clustarry[i].NumOfDOFs; continue; }
            int compactT = nnoze/state().numIntegrationPointsUnitSphere_;
            for(k=0; k<state().clustarry[i].NumOfEl; k++)
            { checkpoint(); 
                k1 = state().clustarry[i].NumsOfEl[k];
                state().jcoltmtx[compactT++] = state().jelist[k1][0]; ++nnoze;
                if(idfs == 2) { state().jcoltmtx[compactT++] = state().jelist[k1][1]; ++nnoze; }
            }
        }
    }
    state().irowtmtx[inoze] = nnoze;
    

    
    state().maxRowNumberD_ = nrowdmtx = state().numOriginalClusters_*state().numIntegrationPointsUnitSphere_; 
    nnonzerdmtx = 0; 
    for(i=0; i<state().numOriginalClusters_; i++)
    { checkpoint(); 
        nnonzerdmtx = index_sum(nnonzerdmtx,state().clustarry[i].NumFarClus);
    }
    nnonzerdmtx = index_product(nnonzerdmtx,state().numIntegrationPointsUnitSphere_);

    
    state().dmtxlev[0].jcolDmxLv = nullptr;
    state().dmtxlev[0].irowDmxLv = allocate_array<int>((nrowdmtx + 1));

    irowd = nnze = 0;
    for(i=0; i<state().numOriginalClusters_; i++)
    { checkpoint(); 
        for(k=0; k<state().numIntegrationPointsUnitSphere_; k++)
        { checkpoint(); 
            state().dmtxlev[0].irowDmxLv[irowd++] =  nnze;
            nnze += state().clustarry[i].NumFarClus;
        }
    }
    state().dmtxlev[0].irowDmxLv[irowd] =  nnze;

    
    state().dmtxlev[0].nRowsD = state().numOriginalClusters_*state().numIntegrationPointsUnitSphere_;
    state().dmtxlev[0].nEntriesD = state().dmtxlev[0].irowDmxLv[state().numOriginalClusters_*state().numIntegrationPointsUnitSphere_];
    

    
    
    Vector<int> nuclusrow(state().numRowsOfCoefficientMatrix_, -1);
    for(i=0; i<state().numOriginalClusters_; i++)
    { checkpoint(); 
        for(j=0; j<state().clustarry[i].NumOfEl; j++)
        { checkpoint(); 
            ij = state().clustarry[i].NumsOfEl[j];
            nuclusrow[state().jelist[ij][0]] = i;
            if(state().clustarry[i].NDOFsPeEl == 2) nuclusrow[state().jelist[ij][1]] = i;
        }
    }

    
    nnonzersmtx = 0;
    for(i=0; i<state().numRowsOfCoefficientMatrix_; i++) if(nuclusrow[i] >= 0) nnonzersmtx += state().numIntegrationPointsUnitSphere_;

    
    state().jcolsmtx = allocate_array<int>((state().numRowsOfCoefficientMatrix_));
    state().irowsmtx = allocate_array<int>((state().numRowsOfCoefficientMatrix_ + 1));

    
    itr = 0;
    for(i=0; i<state().numRowsOfCoefficientMatrix_; i++)
    { checkpoint(); 
        state().irowsmtx[i] = itr;
        if(nuclusrow[i] >= 0)
        {
            kcol = state().numIntegrationPointsUnitSphere_*nuclusrow[i];
            state().jcolsmtx[i] = kcol-itr; itr += state().numIntegrationPointsUnitSphere_;
        }
    }
    state().irowsmtx[state().numRowsOfCoefficientMatrix_] = itr;
    

    
    int sum_nonzero = state().irownea[state().numRowsOfCoefficientMatrix_] + state().irowsmtx[state().numRowsOfCoefficientMatrix_] +
    state().dmtxlev[0].nEntriesD + state().irowtmtx[state().numOriginalReflectedClusters_*state().numIntegrationPointsUnitSphere_];

    NCout << endl;
    NCout << "Nonzeros of the near field matrix ..................... = " <<
    state().irownea[state().numRowsOfCoefficientMatrix_] << endl;
    NCout << "Nonzeros of the T-matrix .............................. = " <<
    state().irowtmtx[state().numOriginalReflectedClusters_*state().numIntegrationPointsUnitSphere_] << endl;
    NCout << "Nonzeros of the D-matrix .............................. = " <<
    state().dmtxlev[0].nEntriesD << endl;
    NCout << "Nonzeros of the S-matrix .............................. = " <<
    state().irowsmtx[state().numRowsOfCoefficientMatrix_] << endl;
    NCout << "Sum of nonzeros of the FMBEM matrices ................. = " <<
    sum_nonzero << endl;
}


void NC_AllocateSDTmtxsMLFMM
(
	ofstream& NCout
)
{
	
	NC_AllocateTmtxMLFMM(NCout);

	
	NC_AllocateDmtxMLFMM(NCout);

	
	NC_AllocateSmtxMLFMM(NCout);

	
    int i, sum_d = 0, sum_t = 0, sum_s = 0;

    for(i=0; i<state().numClusterLevels_; i++)
    { checkpoint(); 
        sum_t += state().tmtxlev[i].nEntriesT;
        sum_d += state().dmtxlev[i].nEntriesD;
        sum_s += state().smtxlev[i].nEntriesS;
    }

    NCout << endl;

    NCout << "Nonzeros of the near field matrix ..................... = " <<
        state().numNonZeroEntries_ << endl;

    NCout << "Nonzeros of the T-matrix .............................. = " <<
        sum_t << endl;
    for(i=0; i<state().numClusterLevels_; i++)
    { checkpoint(); 
        NCout << "    Nonzeros of T_lev_" << i <<
            " ............................... = " << state().tmtxlev[i].nEntriesT << endl;
    }

    NCout << "Nonzeros of the D-matrices ............................ = " <<
        sum_d << endl;
    for(i=0; i<state().numClusterLevels_; i++)
    { checkpoint(); 
        NCout << "    Nonzeros of D_lev_" << i <<
            " ............................... = " << state().dmtxlev[i].nEntriesD << endl;
    }

    NCout << "Nonzeros of the S-matrix .............................. = " <<
        sum_s << endl;
    for(i=0; i<state().numClusterLevels_; i++)
    { checkpoint(); 
        NCout << "    Nonzeros of S_lev_" << i <<
            " ............................... = " << state().smtxlev[i].nEntriesS << endl;
    }

    NCout << "Sum of nonzeros of the FMBEM matrices ................. = " <<
        state().numNonZeroEntries_ + sum_t + sum_d + sum_s << endl;
}


void NC_AllocateTmtxMLFMM
(
	ofstream& NCout
)
{
	int nlv, inoze, nnoze, i, j, k, k1, idfs;

	
	for(nlv=0; nlv<state().numClusterLevels_; nlv++)
	{ checkpoint(); 
		
		j = 0;
		for(i=0; i<state().clulevarry[nlv].nClustSLv; i++)
		{ checkpoint(); 
			j += state().clulevarry[nlv].ClastArLv[i].NumOfDOFs;

		}
		state().tmtxlev[nlv].nEntriesT = index_product(j,state().clulevarry[nlv].nPoinSpheLv);
		state().tmtxlev[nlv].nRowsT = state().clulevarry[nlv].nClustSLv*state().clulevarry[nlv].nPoinSpheLv;

		if(runtime->compactFar){state().tmtxlev[nlv].jcolTmxLv=nullptr;state().tmtxlev[nlv].irowTmxLv=nullptr;continue;}
		state().tmtxlev[nlv].jcolTmxLv = allocate_array<int>((state().tmtxlev[nlv].nEntriesT));
		state().tmtxlev[nlv].irowTmxLv = allocate_array<int>((state().tmtxlev[nlv].nRowsT + 1));

		inoze = nnoze = 0;
		for(i=0; i<state().clulevarry[nlv].nClustSLv; i++)
		{ checkpoint(); 
			idfs = state().clulevarry[nlv].ClastArLv[i].NDOFsPeEl;
			for(j=0; j<state().clulevarry[nlv].nPoinSpheLv; j++)
			{ checkpoint(); 
				state().tmtxlev[nlv].irowTmxLv[inoze++] = nnoze;
				for(k=0; k<state().clulevarry[nlv].ClastArLv[i].NumOfEl; k++)
				{ checkpoint(); 
					k1 = state().clulevarry[nlv].ClastArLv[i].NumsOfEl[k];
					state().tmtxlev[nlv].jcolTmxLv[nnoze++] = state().jelist[k1][0];
					if(idfs == 2) state().tmtxlev[nlv].jcolTmxLv[nnoze++] = state().jelist[k1][1];
				}
			}
		}
		state().tmtxlev[nlv].irowTmxLv[inoze] = nnoze;
	} 
}


void NC_AllocateDmtxMLFMM
(
	ofstream& NCout
 )
{
    int i, j, k, nnze, irowd, nlv, nrosdmtx, nnonzerdmtx;

    Vector<int> nclufar(state().numOriginalClusters_);
    index_product(state().numOriginalClusters_,state().numOriginalReflectedClusters_);
    Matrix<int> nuclfar(state().numOriginalClusters_, state().numOriginalReflectedClusters_);

    
    state().maxRowNumberD_ = 0;
    for(nlv=0; nlv<state().numClusterLevels_; nlv++)
    { checkpoint(); 
        
        if(nlv == 0) 
        {
            for(i=0; i<state().clulevarry[nlv].nClustOLv; i++)
            { checkpoint(); 
                nclufar[i] = state().clulevarry[nlv].ClastArLv[i].NumFarClus;
                for(j=0; j<nclufar[i]; j++)
                    nuclfar(i,j) = state().clulevarry[nlv].ClastArLv[i].NumsFarClus[j];
            }
        }
        else 
        {
            int nlf = nlv-1, ifa, jfa, jself;
            int ncluof = state().clulevarry[nlf].nClustOLv,
            nclusf = state().clulevarry[nlf].nClustSLv;
            Matrix<bool> ifnear(ncluof, nclusf, false);

            for(i=0; i<ncluof; i++)
            { checkpoint(); 
                for(j=0; j<state().clulevarry[nlf].ClastArLv[i].NumNeaClus; j++)
                    ifnear(i, state().clulevarry[nlf].ClastArLv[i].NumsNeaClus[j]) = true;
            }

            for(i=0; i<state().clulevarry[nlv].nClustOLv; i++)
            { checkpoint(); 
                ifa = state().clulevarry[nlv].ClastArLv[i].nuFather;
                nclufar[i] = 0;
                for(j=0; j<state().clulevarry[nlv].ClastArLv[i].NumFarClus; j++)
                { checkpoint(); 
                    jself = state().clulevarry[nlv].ClastArLv[i].NumsFarClus[j];
                    jfa = state().clulevarry[nlv].ClastArLv[jself].nuFather;

                    if(ifnear(ifa, jfa))
                    {
                        nuclfar(i, nclufar[i]++) = jself;
                    }
                }
            }
        } 

        for(i=0; i<state().clulevarry[nlv].nClustOLv; i++)
        { checkpoint(); 
            state().clulevarry[nlv].ClastArLv[i].NumFanClus = nclufar[i];

            state().clulevarry[nlv].ClastArLv[i].NumsFanClus = allocate_array<int>((nclufar[i]));
            for(j=0; j<nclufar[i]; j++)
                state().clulevarry[nlv].ClastArLv[i].NumsFanClus[j] = nuclfar(i, j);
        }

        nrosdmtx = state().clulevarry[nlv].nClustOLv*state().clulevarry[nlv].nPoinSpheLv;
        nnonzerdmtx = 0;
        for(i=0; i<state().clulevarry[nlv].nClustOLv; i++) nnonzerdmtx = index_sum(nnonzerdmtx,nclufar[i]);
        nnonzerdmtx = index_product(nnonzerdmtx,state().clulevarry[nlv].nPoinSpheLv);

        if(runtime->compactFar){state().dmtxlev[nlv].jcolDmxLv=nullptr;state().dmtxlev[nlv].irowDmxLv=nullptr;state().dmtxlev[nlv].nRowsD=nrosdmtx;state().dmtxlev[nlv].nEntriesD=nnonzerdmtx;continue;}
        state().dmtxlev[nlv].jcolDmxLv = allocate_array<int>((nnonzerdmtx));
        state().dmtxlev[nlv].irowDmxLv = allocate_array<int>((nrosdmtx + 1));

        irowd = nnze = 0;
        for(i=0; i<state().clulevarry[nlv].nClustOLv; i++)
        { checkpoint(); 
            for(k=0; k<state().clulevarry[nlv].nPoinSpheLv; k++)
            { checkpoint(); 
                state().dmtxlev[nlv].irowDmxLv[irowd++] =  nnze;
                for(j=0; j<nclufar[i]; j++)
                    state().dmtxlev[nlv].jcolDmxLv[nnze++] =
                    nuclfar(i,j)*state().clulevarry[nlv].nPoinSpheLv + k;
            }
        }

        state().dmtxlev[nlv].irowDmxLv[irowd] =  nnze;
        if(state().maxRowNumberD_ < nrosdmtx) state().maxRowNumberD_ = nrosdmtx;
        state().dmtxlev[nlv].nRowsD = nrosdmtx;
        state().dmtxlev[nlv].nEntriesD = nnonzerdmtx;
    } 
}


void NC_AllocateSmtxMLFMM
(
	ofstream& NCout
)
{
	int nlv, i, j, ij, kcol, itr;
	Vector<int> nuclusrow(state().numRowsOfCoefficientMatrix_);

	
	for(nlv=0; nlv<state().numClusterLevels_; nlv++)
	{ checkpoint(); 
		
		for(i=0; i<state().numRowsOfCoefficientMatrix_; i++) nuclusrow[i] = -1;

		
		for(i=0; i<state().clulevarry[nlv].nClustOLv; i++)
		{ checkpoint(); 
			for(j=0; j<state().clulevarry[nlv].ClastArLv[i].NumOfEl; j++)
			{ checkpoint(); 
				ij = state().clulevarry[nlv].ClastArLv[i].NumsOfEl[j];
				nuclusrow[state().jelist[ij][0]] = i;
				if(state().clulevarry[nlv].ClastArLv[i].NDOFsPeEl == 2)
					nuclusrow[state().jelist[ij][1]] = i;
			}
		}

		
		state().smtxlev[nlv].nEntriesS = 0;
		for(i=0; i<state().numRowsOfCoefficientMatrix_; i++)
		{ checkpoint(); 
			if(nuclusrow[i] >= 0) state().smtxlev[nlv].nEntriesS += state().clulevarry[nlv].nPoinSpheLv;
		}

		
		if(runtime->compactFar){state().smtxlev[nlv].jcolSmxLv=nullptr;state().smtxlev[nlv].irowSmxLv=nullptr;continue;}
		state().smtxlev[nlv].jcolSmxLv = allocate_array<int>((state().smtxlev[nlv].nEntriesS));
		state().smtxlev[nlv].irowSmxLv = allocate_array<int>((state().numRowsOfCoefficientMatrix_ + 1));

		
		itr = 0;
		for(i=0; i<state().numRowsOfCoefficientMatrix_; i++)
		{ checkpoint(); 
			state().smtxlev[nlv].irowSmxLv[i] = itr;
			if(nuclusrow[i] >= 0)
			{
				kcol = state().clulevarry[nlv].nPoinSpheLv*nuclusrow[i];
				for(j=0; j<state().clulevarry[nlv].nPoinSpheLv; j++)
					state().smtxlev[nlv].jcolSmxLv[itr++] = kcol++;
			}
		}
		state().smtxlev[nlv].irowSmxLv[state().numRowsOfCoefficientMatrix_] = itr;
	} 
}


void NC_AllocateNmtx
(
	ofstream& NCout
 )
{



Vector<int> starts(state().numOriginalClusters_ + 1, 0);
Vector<bool> adjacent(state().numOriginalClusters_);
auto mark = [&](int cluster) {
    adjacent = false;
    const auto &c = state().ClustArray[cluster];
    for (int k=0; k<c.NumNeaClus; ++k) { checkpoint(); 
        adjacent[state().ClustArray[c.NumsNeaClus[k]].OriClust] = true;
    }
};
for (int c=0; c<state().numOriginalClusters_; ++c) { checkpoint(); 
    mark(c);
    int width = 0;
    for (int j=0; j<state().numOriginalClusters_; ++j) { checkpoint(); 
        if (adjacent[j]) width += state().ClustArray[j].NumOfDOFs;
    }
    if (width > INT_MAX - starts[c]) throw Failure(3,"Near field column count exceeds limit");
    starts[c+1] = starts[c]+width;
}
state().jcolnea = allocate_array<int>((starts[state().numOriginalClusters_]));
state().irownea = allocate_array<int>((state().numRowsOfCoefficientMatrix_+1));
state().nearColumnBias = allocate_array<int>((state().numRowsOfCoefficientMatrix_));
std::fill_n(state().nearColumnBias,state().numRowsOfCoefficientMatrix_,-1);
for (int c=0; c<state().numOriginalClusters_; ++c) { checkpoint(); 
    mark(c);
    int dst=starts[c];
    
    
    for (int j=0; j<state().numOriginalClusters_; ++j) { checkpoint(); 
        if (!adjacent[j]) continue;
        const auto &n=state().ClustArray[j];
        for (int k=0; k<n.NumOfEl; ++k) { checkpoint(); 
            const int el=n.NumsOfEl[k];
            for (int d=0; d<n.NDOFsPeEl; ++d) { checkpoint(); 
                if (dst >= starts[c+1]) throw Failure(1,"Invalid cluster DOF count");
                state().jcolnea[dst++]=state().jelist[el][d];
            }
        }
    }
    if (dst != starts[c+1]) throw Failure(1,"Invalid near field column count");
    const auto &owner=state().ClustArray[c];
    for (int k=0; k<owner.NumOfEl; ++k) { checkpoint(); 
        for (int d=0; d<owner.NDOFsPeEl; ++d) { checkpoint(); 
            const int row=state().jelist[owner.NumsOfEl[k]][d];
            if (row<0 || row>=state().numRowsOfCoefficientMatrix_ || state().nearColumnBias[row]!=-1)
                throw Failure(1,"Invalid near field row owner");
            state().nearColumnBias[row]=c;
        }
    }
}
state().numNonZeroEntries_=0;
for (int row=0; row<state().numRowsOfCoefficientMatrix_; ++row) { checkpoint(); 
    const int c=state().nearColumnBias[row];
    if (c<0) throw Failure(1,"Missing near field row owner");
    state().irownea[row]=state().numNonZeroEntries_;
    state().nearColumnBias[row]=starts[c]-state().numNonZeroEntries_;
    const int width=starts[c+1]-starts[c];
    if (width > INT_MAX-state().numNonZeroEntries_) throw Failure(3,"Near field exceeds solver dimensions");
    state().numNonZeroEntries_+=width;
}
state().irownea[state().numRowsOfCoefficientMatrix_]=state().numNonZeroEntries_;
state().boolComputeTVector_=false;
for (int c=0; c<state().numOriginalReflectedClusters_; ++c) { checkpoint(); 
    if (state().ClustArray[c].listElementPropertyEl==0 && state().ibval[state().ClustArray[c].NumsOfEl[0]]<3) {
        state().boolComputeTVector_=true; break;
    }
}

}


void NC_CheckClusters
(
	ofstream& NCout,
	const int& n_levl,			
	const int& n_clustero,		
	ElCluster *clust_arry		
)
{
	int i, j, nfalsregi = 0;
	Vector<int> ntimeselregi(state().numElements_, 0);

	
	for(i=0; i<n_clustero; i++)
	{ checkpoint(); 
		for(j=0; j<clust_arry[i].NumOfEl; j++)
		{ checkpoint(); 
			ntimeselregi[clust_arry[i].NumsOfEl[j]]++;
		}

	}

	
	for(i=0; i<state().numElements_; i++)
	{ checkpoint(); 
		if(state().listElementProperty[i] == 2) continue;
		if(ntimeselregi[i] == 0)
		{
			nfalsregi++;
			NCout << "\nThe element " << state().extNumbersOfElements[i] << " is not included in any cluster"
				<< endl;
			runtime->quiet << "\nThe element " << state().extNumbersOfElements[i] << " is not included in any cluster"
				<< endl;
		}
		else if(ntimeselregi[i] > 1)
		{
			nfalsregi++;
			NCout << "\nThe element " << state().extNumbersOfElements[i] << " is included in " << ntimeselregi[i]
				<< " clusters" << endl;
			runtime->quiet << "\nThe element " << state().extNumbersOfElements[i] << " is included in " << ntimeselregi[i]
				<< " clusters" << endl;
		}
	}

	
	if(nfalsregi > 0)
	{
		NC_Error_Exit_2(NCout,
			"Elements are not or more than one times included by clusters",
			"level number of the clusters = ", n_levl,
			"number of such elements      = ", nfalsregi);
	}
}


void NC_DeleteArrays(const int& imultpol, const int& nlevmlfm, const int& ifdelNFC)
{
	int i, j;

	
	switch(imultpol)
	{
	case 1:
	case 2:
		release_array(state().dmtxlev);
		break;
	case 3:
		release_array(state().dmtxlev);
		release_array(state().tmtxlev);
		release_array(state().smtxlev);
		break;
	}

	if(imultpol && state().numInternalPointsClusters_ > 0)
	{
		for(i=0; i<state().numInternalPointsClusters_; i++) release_array(state().ipcluarry[i].NumsOfIps);
		release_array(state().ipcluarry);
	}

	switch(imultpol)
	{
	case 1: 
		for(i=0; i<state().numOriginalReflectedClusters_; i++)
		{ checkpoint(); 
			release_array(state().clustarry[i].NumsFarClus);
			release_array(state().clustarry[i].NumsNeaClus);
			release_array(state().clustarry[i].NumsOfEl);
		}
		release_array(state().clustarry);
		break;
	case 3: 
	  for(i=0; i<nlevmlfm; i++)
	    { checkpoint(); 
	      for(j=0; j<state().clulevarry[i].nClustSLv; j++)
		{ checkpoint(); 
		  
		  
		  if( state().clulevarry[i].ClastArLv[j].NumsFarClus != NULL )
		    release_array(state().clulevarry[i].ClastArLv[j].NumsFarClus);
		  release_array(state().clulevarry[i].ClastArLv[j].NumsNeaClus);
		  release_array(state().clulevarry[i].ClastArLv[j].NumsOfEl);
		}
	      
	      if(ifdelNFC)
		{
		  for(j=0; j<state().clulevarry[i].nClustOLv; j++)
		    { checkpoint(); 
		      release_array(state().clulevarry[i].ClastArLv[j].NumsFanClus);
		    }
		}
	      
	      release_array(state().clulevarry[i].ClastArLv);
	      
	    }
	  release_array(state().clulevarry);
	  break;
	}

	if(imultpol) 
	{
		release_array(state().nearColumnBias); release_array(state().jcolnea);
		release_array(state().irownea);
	}

	release_array(state().zrhs);
}












void NC_CancelSmallClusters
(
 Vector<int>& ne_clus,	
 Vector<int>& nue_clus,	
 int& nclus,				
 int& ibg                
 )
{
  int i, j, k, i1, iel, ine, nel_max = 0, n_nod, l1, m;
  double dwk, dmin, threshfac1 = 3.0, dispo, rdmax, rdmin, rdaver;
  bool iffewel = false;

  
  for(i=0; i<nclus; i++) if(ne_clus[i] > nel_max) nel_max = ne_clus[i];

  
  Vector<int> nu_nod_cl(nel_max*NNODPE);

  
  Vector<double> radi_cl(nclus);

  
  double **crdcenters = allocate_array<double*>((nclus));
  
  if(!crdcenters) {
    fprintf(stderr,"Could not allocate crdcenters\n");
    fail_exit(-1);
  }
  for(i=0; i<nclus; i++) { checkpoint(); 
    crdcenters[i] = allocate_array<double>((NDIM));
    if(!crdcenters[i]) {
      fprintf(stderr,"Could not allocate crdcenters[i]\n");
      fail_exit(-1);
    }
  }
  for(i=0; i<nclus; i++) for(j=0; j<NDIM; j++) crdcenters[i][j] = 0.0;

  
  i1 = 0;
  for(i=0; i<nclus; i++)
    { checkpoint(); 
      if(ne_clus[i] == 0) continue;

      
      n_nod = 0;
      for(j=0; j<ne_clus[i]; j++)
	{ checkpoint(); 
	  iel = nue_clus[i1++];
	  for(k=0; k<state().listNumberNodesPerElement[iel]; k++)
	    { checkpoint(); 
	      l1 = state().elementsConnectivity[iel][k];
	      for(m=0; m<n_nod; m++) if(nu_nod_cl[m] == l1) goto LbclustAray1;
	      nu_nod_cl[n_nod++] = l1;
	    LbclustAray1: continue;
	    }
	}

      
      for(j=0; j<n_nod; j++)
	{ checkpoint(); 
	  for(k=0; k<NDIM; k++) crdcenters[i][k] += state().nodesCoordinates[nu_nod_cl[j]][k];
	}
      for(k=0; k<NDIM; k++) crdcenters[i][k] /= (double)n_nod;

      
      radi_cl[i] = 0.0;
      for(j=0; j<n_nod; j++)
	{ checkpoint(); 
	  dispo = Dispoi_dim3_(crdcenters[i], state().nodesCoordinates[nu_nod_cl[j]]);
	  if(dispo > radi_cl[i]) radi_cl[i] = dispo;
	}
    } 

  
  rdmax = rdaver = i1 = 0;
  rdmin = 1.0e20;
  for(i=0; i<nclus; i++)
    { checkpoint(); 
      if(ne_clus[i] == 0) continue;
      rdaver += radi_cl[i];
      i1++;
      if(radi_cl[i] < rdmin) rdmin = radi_cl[i];
      if(radi_cl[i] > rdmax) rdmax = radi_cl[i];
    }
  rdaver /= (double)i1;

  
  if(rdaver > threshfac1*rdmin) iffewel = true;

  if(!iffewel) {
    for(i=0; i<nclus; i++) release_array(crdcenters[i]);
    release_array(crdcenters);
    return;
  }

  
  Vector<int> NearClus(nclus, -1);
  for(i=0; i<nclus; i++)
    { checkpoint(); 
      if(ne_clus[i] == 0 || threshfac1*radi_cl[i] >= rdaver) continue;
      dmin = 1.0e20;
      for(j=0; j<nclus; j++)
	{ checkpoint(); 
	  if(i == j || ne_clus[j] == 0 || threshfac1*radi_cl[j] < rdaver) continue;
	  dwk = Dispoi_dim3_(crdcenters[i], crdcenters[j]);
	  if(dwk < dmin)
	    {
	      dmin = dwk;
	      NearClus[i] = j;
	    }
	}
    }

  
  int jdimtx = nel_max;
  for(i=0; i<nclus; i++)
    { checkpoint(); 
      if(NearClus[i] >= 0)
	{

	  
	  jdimtx = jdimtx + ne_clus[i];
	}
    }
  Matrix<int> NuElClust(nclus, jdimtx, -1);
  i1 = 0;
  for(i=0; i<nclus; i++)
    { checkpoint(); 
      if(ne_clus[i] == 0) continue;
      for(j=0; j<ne_clus[i]; j++) NuElClust(i, j) = nue_clus[i1++];
    }

  
  for(i=0; i<nclus; i++)
    { checkpoint(); 
      if(NearClus[i] == -1) continue;

      ine = NearClus[i];
      i1 = 0;
      for(j=0; j<jdimtx; j++)
	{ checkpoint(); 
	  if(NuElClust(ine, j) == -1) break;
	  i1++;
	}
      for(j=0; j<ne_clus[i]; j++) NuElClust(ine, i1 + j) = NuElClust(i, j);
      ne_clus[ine] += ne_clus[i];
      ne_clus[i] = 0;
    }

  i1 = 0;
  for(i=0; i<nclus; i++)
    { checkpoint(); 
      if(ne_clus[i] == 0) continue;
      for(j=0; j<ne_clus[i]; j++) nue_clus[i1++] = NuElClust(i, j);
    }

  
  for(i=0; i<nclus; i++) release_array(crdcenters[i]);
  release_array(crdcenters);
}

