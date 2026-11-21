









using namespace std;                                                                





void NC_BuildSystemTBEM(ofstream&);
void NC_AssembleTBEM(ofstream&,const int&,Vector<int>&,const int&);
void NC_ComputeEntriesForTBEM(ofstream&,const int&);
void NC_BuildSystemFMBEM(ofstream&);
void NC_BuildTforSLFMM(ofstream&);
void NC_BuildDforSLFMM(ofstream&);
void NC_BuildSforSLFMM(ofstream&);
void NC_BuildNforSLFMM(ofstream&);
void NC_BuildTforMLFMM(ofstream&);
void NC_BuildDforMLFMM(ofstream&);
void NC_BuildSforMLFMM(ofstream&);
void NC_BuildNforMLFMM(ofstream&);
void NC_RHSvecTofSLFMM(ofstream&);
void NC_RHSvecTofMLFMM(ofstream&);
void NC_ComputeEntriesTforFMM(ofstream&,const int&,const int&,int&,int&,double*,double**,Vector<Complex>&,Vector<Complex>&);
void NC_ComputeEntriesNforFMM(ofstream&,const int&,const int&,Vector<int>&);
void NC_AssembleNforFMM(ofstream&,const int&,const int&,Vector<int>&,int&);
void NC_SearchAddressNforFMM(ofstream&,int&,int&,int&,const char&);
void NC_SingularIntegration(ofstream&,Vector<Complex>&,const int&,const int&,Vector<Complex>&,Matrix<double>&);
void NC_RegularIntegration(ofstream&,Vector<Complex>&,const int&,const int&,const int&,const int&,Vector<Complex>&,Vector<double>&,Matrix<double>&);
Complex NC_IncidentWaveRHS(ofstream&);
double NC_FreqCurveInterpolation(ofstream&, int&);









 










const int GauFMBOrd3 = 2;




void NC_SetupEquationSystem
(
	ofstream& NCout
)
{
	
	state().Gama3 = 1.0;
	if(state().isInternalProblem_) {state().zBta3.set(0.0, 0.0);} else {state().zBta3.set(0.0, state().harmonicTimeFactor_/state().waveNumbers_);}

	switch(state().methodFMM_)
	{
	case 0: 
		NC_BuildSystemTBEM(NCout);
		break;
	case 1: 
	case 3: 
		NC_BuildSystemFMBEM(NCout);
	}
}


void NC_BuildSystemTBEM
(
	ofstream& NCout
)
{
	int i, iel;

	
	for(i=0; i<state().numComponentsOfCoefficientMatrix_; i++) state().zcoefl[i].set(0.0, 0.0);

	
	for(iel=0; iel<state().numElements_; iel++)
	{ checkpoint(); 
		if(state().listElementProperty[iel] == 2) continue;

		
		for(i=0; i<NDIM; i++)
		{ checkpoint(); 
			state().Sourpoi3[i] = state().centel[iel][i];
			state().Norvci3[i] = state().elenor[iel][i];
		}

		
		state().Souradr3[0] = state().jelist[iel][0];
		state().Souradr3[1] = state().jelist[iel][1];

		
		switch(state().ibval[iel]) 
		{
		case 0:         
			state().Ibvi03 = 0;  
			state().Ifadmii_3 = 0;
			break;
		case 1:         
			state().Ibvi03 = 1;
			state().Ifadmii_3 = 0;
			break;
		case 2:         
			state().Ibvi03 = 0;  
			state().Ifadmii_3 = 1;
			break;
		case 3:         
			state().Ibvi03 = 2;
			state().Ifadmii_3 = 0;
			break;
		case 4:         
			state().Ibvi03 = 2;
			state().Ifadmii_3 = 1;
			break;
		}
		state().Zbvi03 = 0.0;
		for(i=0; i<state().listNumberNodesPerElement[iel]; i++) state().Zbvi03 += state().zbval0[iel][i];
		state().Zbvi03 /= (double)state().listNumberNodesPerElement[iel];

		
		if(state().listElementProperty[iel] == 0) 
		{
			if(state().Ifadmii_3) {state().Admia3 = state().zbval1[iel];} else {state().Admia3.set(0.0, 0.0);}
			NC_ComputeEntriesForTBEM(NCout, iel);
		}

	} 
}


void NC_AssembleTBEM
(
	ofstream& NCout,
	const int& jel,			
	Vector<int>& arg_Souradr3,	
	const int& irefel
 )
{
    int ijadr00, ijadr01, ijadr10, ijadr11;
    Complex zsum0, zsum1;
    double reffact;
    
    ijadr00 = arg_Souradr3[0]*state().numRowsOfCoefficientMatrix_ + state().Jdofaddr3[0];
    ijadr01 = arg_Souradr3[0]*state().numRowsOfCoefficientMatrix_ + state().Jdofaddr3[1];
    ijadr10 = arg_Souradr3[1]*state().numRowsOfCoefficientMatrix_ + state().Jdofaddr3[0];
    ijadr11 = arg_Souradr3[1]*state().numRowsOfCoefficientMatrix_ + state().Jdofaddr3[1];
    
    if(irefel == -1) {reffact = -1.0;} else {reffact = 1.0;}
    
    if(state().Ibvj_03 == 0)
    {
        zsum0 = state().Zrsintel[1]*(state().Gama3*state().Tao_) + state().Zrsintel[3]*state().zBta3;
        if(state().Ifadmj3) zsum0 += (state().Zrsintel[0]*state().Gama3 + state().Zrsintel[2]*(state().zBta3*state().Tao_))*state().Admj3[0];
        
        if(state().Ifcrh3)
        {
            state().zrhs[arg_Souradr3[0]] += state().Zrsintel[4];
        }
    }
    else if(state().Ibvj_03 == 1)
    {
        zsum0 = state().Zrsintel[0]*(-state().Gama3*state().Tao_) - state().Zrsintel[2]*(state().zBta3);
        if(state().Ifcrh3)
            state().zrhs[arg_Souradr3[0]] += state().Zrsintel[4];
    }
    else
    {
        NC_Error_Exit_2(NCout, "state().Ibvj_03 must be equal to 0 or 1!", "state().Ibvj_03 = ", state().Ibvj_03,
                        "Element number = ", state().extNumbersOfElements[jel]);
    }
    
    state().zcoefl[ijadr00] += zsum0*reffact;
}


void NC_ComputeEntriesForTBEM
(
	ofstream& NCout, 
	const int& iel           
)
{
	int jel, kref, i, ireffac;
	Complex z0, localAdmittance(0.0, 0.0);

	
	switch(state().Ibvi03)
	{
	case 0:
		state().zcoefl[state().Souradr3[0]*state().numRowsOfCoefficientMatrix_ + state().Souradr3[0]] += (state().Admia3*state().zBta3 - state().Gama3)*0.5;
		break;
	case 1:
		state().zcoefl[state().Souradr3[0]*state().numRowsOfCoefficientMatrix_ + state().Souradr3[0]] -= state().zBta3*(0.5*state().Tao_);
		break;
	default:
		NC_Error_Exit_2(NCout, "state().Ibvi03 must be equal to 0 or 1!", "state().Ibvi03 = ", state().Ibvi03,
			"element number = ", state().extNumbersOfElements[iel]);
	}

	
	z0 = 0.0;

	
	if(state().Ibvi03 == 0) 
	{
		z0 += state().Zbvi03*state().zBta3*state().Tao_*0.5;
    }
    if(state().Ibvi03 == 1) 
    {
        z0 += state().Zbvi03*state().Tao_*0.5;
    }

	
	if(state().numIncidentPlaneWaves_ + state().numPointSources_ > 0)
	{
		z0 += NC_IncidentWaveRHS(NCout);
	}
	state().zrhs[state().Souradr3[0]] += z0;

	
	for(jel=0; jel<state().numElements_; jel++)
	{ checkpoint(); 
		if(state().listElementProperty[jel] == 2) continue;

		
		NC_ComputeElementData(jel, 0, state().Inoj3, state().Ibvj_03,
			state().Ifadmj3, state().Jdofaddr3, state().Centej3, state().Norvcj3, state().Admj3,
			state().Zbvj03, state().Crdej3);

		
		state().Ifcrh3 = 0;
		if((state().Ibvj_03 == 0 || state().Ibvj_03 == 1) && state().listElementProperty[jel] == 0)
		{
			for(i=0; i<state().Inoj3; i++)
			{ checkpoint(); 
				if(state().Zbvj03[i].norm() > EPSY)
				{
					state().Ifcrh3 = 1;
					break;
				}
			}
		}

		
		ireffac = 1;
		for(kref=0; kref<state().numReflectionsOfElements_; kref++)
		{ checkpoint(); 
			
			if(kref > 0) NC_ReflectElement(kref, state().Inoj3, state().Ibvj_03, ireffac, state().Centej3,
				state().Norvcj3, state().Zbvj03, state().Crdej3);

			if(kref == 0 && jel == iel)
			{
				
				NC_SingularIntegration(NCout, state().Zrsintel, state().Inoj3, state().Ifcrh3, state().Zbvj03,
					state().Crdej3);
			}
			else
			{
				
				NC_RegularIntegration(NCout, state().Zrsintel, iel, jel, state().Inoj3,
					state().Ifcrh3, state().Zbvj03, state().Centej3, state().Crdej3);
			}

			
			NC_AssembleTBEM(NCout, jel, state().Souradr3, ireffac);
		} 
	} 
}


void NC_BuildSystemFMBEM
(
	ofstream& NCout
 )
{
    int nlv;
    
    switch(state().methodFMM_)
    {
        case 1: 
            
            NC_BuildNforSLFMM(NCout);
            
            
            NC_ComputeUnitVectorOnUnitSphere(NCout, state().numIntegrationPointsThetaDirection_, state().numIntegrationPointsPhiDirection_, state().weisphe, state().uvcsphe);
            
            
            NC_BuildDforSLFMM(NCout);
            
            
            NC_BuildTforSLFMM(NCout);
            
            
            NC_BuildSforSLFMM(NCout);
            
            break;
        case 3: 
            
            NC_BuildNforMLFMM(NCout);
            
            
            for(nlv=0; nlv<state().numClusterLevels_; nlv++)
            { checkpoint(); 
                NC_ComputeUnitVectorOnUnitSphere(NCout, state().clulevarry[nlv].nPoinThetLv, state().clulevarry[nlv].nPoinPhiLv,
                                                 state().clulevarry[nlv].weisphe, state().clulevarry[nlv].uvcsphe);
            }
            
            
            if(runtime->compactFar)packed_prepare();
            NC_BuildDforMLFMM(NCout);
            
            
            
            
            
            NC_BuildTforMLFMM(NCout);
            
            
            if(!runtime->compactFar)NC_BuildSforMLFMM(NCout);
    }
    
    
    if(state().boolComputeTVector_) 
    {
        switch(state().methodFMM_)
        {
            case 1: 
                NC_RHSvecTofSLFMM(NCout);
                break;
            case 3: 
                NC_RHSvecTofMLFMM(NCout);
                break;
        }
    }
}


void NC_BuildTforSLFMM
(
	ofstream& NCout
 )
{
    int i, j, k, nelj, irownu, ientry, iadrek, ndofi, inod0 = 0, idfs, ncldofi, jdifdar;
    double *cencli;
    int ifvcj, nfmgp;
    
    int nrf_cl, rfa_cl;
    double dfa_cl;
    bool mir_cl;
    Vector<bool> rfdi_cl(NDIM);
    Complex *zt_mtx;
    
    
    Vector<Complex> TVele(state().numIntegrationPointsUnitSphere_), TMele(state().numIntegrationPointsUnitSphere_);
    
    
    if(state().boolComputeTVector_) for(i=0; i<state().numIntegrationPointsUnitSphere_*state().numOriginalReflectedClusters_; i++) state().ztvct[i].set(0.0, 0.0);
    
    
    zt_mtx = state().ztmtx;
    
    
    for(i=0; i<state().irowtmtx[state().numIntegrationPointsUnitSphere_*state().numOriginalReflectedClusters_]; i++) zt_mtx[i].set(0.0, 0.0);
    
    
    irownu = ientry = 0;
    for(i=0; i<state().numOriginalReflectedClusters_; i++)
    { checkpoint(); 
        
        idfs = state().ClustArray[i].NDOFsPeEl;
        nrf_cl = state().ClustArray[i].nuref;
        rfa_cl = state().ClustArray[i].rffac;
        if(nrf_cl)
        {
            mir_cl = state().ClustArray[i].ifmirro;
            for(j=0; j<NDIM; j++) rfdi_cl[j] = state().ClustArray[i].ifrfdi[j];
        }
        
        
        if(state().ClustArray[i].IfMonoEl)
        {
            inod0 = state().listNumberNodesPerElement[state().ClustArray[i].NumsOfEl[0]];
            nfmgp = NC_ComputeGausseanPoints(state().csiFMgau, state().etaFMgau, state().weiFMgau, inod0, GauFMBOrd3);
        }
        
        ndofi = state().ClustArray[i].NDOFsPeEl;   
        ncldofi = state().ClustArray[i].NumOfDOFs; 
        cencli = state().ClustArray[i].CoorCent;   
        
        
        for(j=0; j<state().ClustArray[i].NumOfEl; j++)
        { checkpoint(); 
            nelj = state().ClustArray[i].NumsOfEl[j];
            
            
            if(!state().ClustArray[i].IfMonoEl && state().listNumberNodesPerElement[nelj] != inod0)
            {
                inod0 = state().listNumberNodesPerElement[nelj];
                nfmgp = NC_ComputeGausseanPoints(state().csiFMgau, state().etaFMgau, state().weiFMgau, inod0, GauFMBOrd3);
            }
            
            
            NC_ComputeElementData(nelj, 0, state().Inoj3, state().Ibvj_03,
                                  state().Ifadmj3, state().Jdofaddr3, state().Centej3, state().Norvcj3, state().Admj3,
                                  state().Zbvj03, state().Crdej3);
            
            
            if(nrf_cl) NC_ReflectElementFMM(rfa_cl, mir_cl, rfdi_cl, state().Inoj3, state().Ibvj_03,
                                            state().Centej3, state().Norvcj3, state().Zbvj03, state().Crdej3);
            
            
            
            ifvcj = 0;
            if(state().boolComputeTVector_ && (state().Ibvj_03 == 0 || state().Ibvj_03 == 1))
            {
                for(k=0; k<state().Inoj3; k++) if(state().Zbvj03[k].norm() > 0.0)
                {
                    ifvcj = 1;
                    break;
                }
            }
            
            
            for(k=0; k<state().numIntegrationPointsUnitSphere_; k++)
            { checkpoint(); 
                TMele[k].set(0.0, 0.0);
                TVele[k].set(0.0, 0.0);
            }
            
            if(state().Ifadmj3) {state().Admia3 = state().zbval1[nelj];} else {state().Admia3.set(0.0, 0.0);}
            NC_ComputeEntriesTforFMM(NCout, nelj, ifvcj, state().numIntegrationPointsUnitSphere_, nfmgp,
                          cencli, state().uvcsphe, TMele, TVele);
            
            
            jdifdar = j*ndofi;
            if(rfa_cl == -1) {dfa_cl = -1.0;} else {dfa_cl = 1.0;}
            
            for(k=0; k<state().numIntegrationPointsUnitSphere_; k++)
            { checkpoint(); 
                iadrek = ientry + k*ncldofi + jdifdar;
                
                zt_mtx[iadrek] = TMele[k]*dfa_cl;
                if(idfs == 2)  zt_mtx[++iadrek] = TVele[k]*dfa_cl;
                
                if(ifvcj) state().ztvct[irownu + k] += TVele[k];
            }
        } 
        irownu += state().numIntegrationPointsUnitSphere_;
        ientry += state().numIntegrationPointsUnitSphere_*ncldofi;
    } 
}



void NC_BuildDforSLFMM
(
	ofstream& NCout
)
{
	int i, j, jcl, k, jgp, identry, ikjadre;
	double dij, dkij, scprd;
	Complex zmun, zi;
	Vector<Complex> zH1m(state().numExpansionTerms_), zdiat(state().numIntegrationPointsUnitSphere_);
	Vector<double> Pm(state().numExpansionTerms_), disij(NDIM);
	Complex *zwork_d;

	
	zwork_d = state().dmtxlev[0].zDmxLv;

	
	identry = 0; 

	for(i=0; i<state().numOriginalClusters_; i++) 
	{ checkpoint(); 
		
		for(j=0; j<state().ClustArray[i].NumFarClus; j++)
		{ checkpoint(); 
			jcl = state().ClustArray[i].NumsFarClus[j];
			for(k=0; k<NDIM; k++) disij[k] = state().ClustArray[i].CoorCent[k] - 
				state().ClustArray[jcl].CoorCent[k];
			Vcnorm_dim3_(disij, dij);
			dkij = dij*state().waveNumbers_;

			
			NC_SphericalHankel(NCout, zH1m, state().numExpansionTerms_, dkij);

			for(jgp=0; jgp<state().numIntegrationPointsUnitSphere_; jgp++)
			{ checkpoint(); 
				scprd = Scprod_dim3_(disij, state().uvcsphe[jgp]);

				
				NC_LegendrePolynomes(NCout, Pm, state().numExpansionTerms_, scprd);

				
				zmun.set(0.0, 0.0);
				zi.set(1.0, 0.0);
				for(k=0; k<state().numExpansionTerms_; k++)
				{ checkpoint(); 
					zmun += zi*zH1m[k]*Pm[k]*(double)(2*k + 1);
					zi.mul_i(state().harmonicTimeFactor_); 
				}
				zdiat[jgp] = zmun;
			} 

			
			for(k=0; k<state().numIntegrationPointsUnitSphere_; k++)
			{ checkpoint(); 
				ikjadre = identry + k*state().ClustArray[i].NumFarClus + j;
				zwork_d[ikjadre] = zdiat[k];
			}
		} 
		identry += state().numIntegrationPointsUnitSphere_*state().ClustArray[i].NumFarClus;
	} 
}


void NC_BuildSforSLFMM
(
	ofstream& NCout
)
{
	int icl, iel, nel, i, jip, iterm0;
	double scpr_dv, d0, wavruim = state().waveNumbers_*state().harmonicTimeFactor_;
	Complex z0;
	Vector<double> dcenel(NDIM);
	double *norel;
	bool ifadm;
	Vector<Complex> zvcun(state().numIntegrationPointsUnitSphere_), zvcdun(state().numIntegrationPointsUnitSphere_);
	Complex *zs_mtx;

	
	zs_mtx = state().zsmtx;

	
	for(icl=0; icl<state().numOriginalClusters_; icl++)
	{ checkpoint(); 
		ifadm = state().ClustArray[icl].IfAdmiBc;

		
		for(iel=0; iel<state().ClustArray[icl].NumOfEl; iel++)
		{ checkpoint(); 
			nel = state().ClustArray[icl].NumsOfEl[iel]; 

			
			for(i=0; i<NDIM; i++) dcenel[i] = state().centel[nel][i] - state().ClustArray[icl].CoorCent[i];

			
			norel = state().elenor[nel];

			
			for(jip=0; jip<state().numIntegrationPointsUnitSphere_; jip++)
			{ checkpoint(); 
				
				scpr_dv = Scprod_dim3_(dcenel, state().uvcsphe[jip]);

				
				d0 = scpr_dv*wavruim;
				z0.set(cos(d0), sin(d0));
				z0 *= state().weisphe[jip];
				zvcun[jip] = z0;

				d0 = Scprod_dim3_(state().uvcsphe[jip], norel);
				d0 *= wavruim;
				z0.mul_i(d0);
				zvcdun[jip] = z0;
			} 

			
			iterm0 = state().irowsmtx[state().jelist[nel][0]];
        
            for(i=0; i<state().numIntegrationPointsUnitSphere_; i++)
            { checkpoint(); zs_mtx[iterm0 + i] = zvcun[i]*state().Gama3 + zvcdun[i]*(state().zBta3*state().Tao_);}

		} 
	} 
}


void NC_BuildNforSLFMM
(
	ofstream& NCout
 )
{
    int i, ic, ie, iel, k, k1, nori_clu;
    Vector<bool> bneacluso(state().numOriginalClusters_);
    Vector<int> iadreclu(state().numOriginalClusters_);
    
    
    for(i=0; i<state().irownea[state().numRowsOfCoefficientMatrix_]; i++) state().zcoefl[i].set(0.0, 0.0);
    
    
    for(ic=0; ic<state().numOriginalClusters_; ic++)
    { checkpoint(); 
        
        bneacluso = false;
        for(k=0; k<state().ClustArray[ic].NumNeaClus; k++)
        { checkpoint(); 
            nori_clu = state().ClustArray[state().ClustArray[ic].NumsNeaClus[k]].OriClust;
            if(!bneacluso[nori_clu]) bneacluso[nori_clu] = true;
        }
        
        
        iadreclu = -1;
        k1 = 0;
        for(k=0; k<state().numOriginalClusters_; k++)
        { checkpoint(); 
            if(!bneacluso[k]) continue;
            iadreclu[k] = k1;
            k1 += state().ClustArray[k].NumOfDOFs;
        }
        
        
        for(ie=0; ie<state().ClustArray[ic].NumOfEl; ie++)
        { checkpoint(); 
            
            iel = state().ClustArray[ic].NumsOfEl[ie];
            
            
            for(i=0; i<NDIM; i++)
            { checkpoint(); 
                state().Sourpoi3[i] = state().centel[iel][i];
                state().Norvci3[i] = state().elenor[iel][i];
            }
            
            
            state().Souradr3[0] = state().jelist[iel][0];
            state().Souradr3[1] = state().jelist[iel][1];
            
            
            switch(state().ibval[iel])
            {
                case 0:         
                    state().Ibvi03 = 0;
                    state().Ifadmii_3 = 0;
                    break;
                case 1:         
                    state().Ibvi03 = 1;
                    state().Ifadmii_3 = 0;
                    break;
                case 2:         
                    state().Ibvi03 = 0;
                    state().Ifadmii_3 = 1;
                    break;
                case 3:         
                    state().Ibvi03 = 2;
                    state().Ifadmii_3 = 0;
                    break;
                case 4:         
                    state().Ibvi03 = 2;
                    state().Ifadmii_3 = 1;
                    break;
            }
            state().Zbvi03 = 0.0;
            for(i=0; i<state().listNumberNodesPerElement[iel]; i++) state().Zbvi03 += state().zbval0[iel][i];
            state().Zbvi03 /= (double)state().listNumberNodesPerElement[iel];
            
            
            if(state().listElementProperty[iel] == 0) 
            {
                if(state().Ifadmii_3) {state().Admia3 = state().zbval1[iel];} else {state().Admia3.set(0.0, 0.0);}
                NC_ComputeEntriesNforFMM(NCout, ic, iel, iadreclu);
            }
        } 
    } 
}


void NC_BuildTforMLFMM
(
	ofstream& NCout
 )
{
    int i, j, k, nelj, irownu, ientry, iadrek = 0, ndofi, inod0 = 0, idfs, ncldofi, jdifdar;
    double *cencli;
    int ifvcj, nfmgp;
    
    int nrf_cl, rfa_cl, nlv, nPoSphe, nClustL, nrowT;
    double dfa_cl;
    bool mir_cl;
    Vector<bool> rfdi_cl(NDIM);
    
    ElCluster *clustArraLn;
    Complex *zTvcLn, *zTmxLn;
    int *irowTmxLn;
    
    
    Vector<Complex> TVele(state().clulevarry[0].nPoinSpheLv), TMele(state().clulevarry[0].nPoinSpheLv);
    
    
    for(nlv=0; nlv<state().numClusterLevels_; nlv++)
    { checkpoint(); 
        
        nPoSphe = state().clulevarry[nlv].nPoinSpheLv;
        nClustL = state().clulevarry[nlv].nClustSLv;
        nrowT = nPoSphe*nClustL;
        clustArraLn = state().clulevarry[nlv].ClastArLv;
        zTvcLn = state().tmtxlev[nlv].zTvcLv;
        zTmxLn = state().tmtxlev[nlv].zTmxLv;
        irowTmxLn = state().tmtxlev[nlv].irowTmxLv;
        
        
        if(state().boolComputeTVector_) for(i=0; i<nrowT; i++) zTvcLn[i].set(0.0, 0.0);
        
        
        if(!runtime->compactFar)for(i=0; i<irowTmxLn[nrowT]; i++) zTmxLn[i].set(0.0, 0.0);
        
        
        irownu = ientry = 0;
        for(i=0; i<nClustL; i++)
        { checkpoint(); 
            
            idfs = clustArraLn[i].NDOFsPeEl;
            nrf_cl = clustArraLn[i].nuref;
            rfa_cl = clustArraLn[i].rffac;
            if(nrf_cl)
            {
                mir_cl = clustArraLn[i].ifmirro;
                for(j=0; j<NDIM; j++) rfdi_cl[j] = clustArraLn[i].ifrfdi[j];
            }
            
            
            if(clustArraLn[i].IfMonoEl)
            {
                inod0 = state().listNumberNodesPerElement[clustArraLn[i].NumsOfEl[0]];
                nfmgp = NC_ComputeGausseanPoints(state().csiFMgau, state().etaFMgau, state().weiFMgau, inod0, GauFMBOrd3);
            }
            
            
            ndofi = clustArraLn[i].NDOFsPeEl;
            ncldofi = clustArraLn[i].NumOfDOFs;
            
            cencli = clustArraLn[i].CoorCent;
            
            
            for(j=0; j<clustArraLn[i].NumOfEl; j++)
            { checkpoint(); 
                nelj = clustArraLn[i].NumsOfEl[j];
                
                
                if(!clustArraLn[i].IfMonoEl && state().listNumberNodesPerElement[nelj] != inod0)
                {
                    inod0 = state().listNumberNodesPerElement[nelj];
                    nfmgp = NC_ComputeGausseanPoints(state().csiFMgau, state().etaFMgau, state().weiFMgau, inod0, GauFMBOrd3);
                }
                
                
                NC_ComputeElementData(nelj, 0, state().Inoj3, state().Ibvj_03,
                                      state().Ifadmj3, state().Jdofaddr3, state().Centej3, state().Norvcj3, state().Admj3,
                                      state().Zbvj03, state().Crdej3);
                
                
                if(nrf_cl) NC_ReflectElementFMM(rfa_cl, mir_cl, rfdi_cl, state().Inoj3, state().Ibvj_03,
                                                state().Centej3, state().Norvcj3, state().Zbvj03, state().Crdej3);
                
                
                ifvcj = 0;
                if(state().boolComputeTVector_ && (state().Ibvj_03 == 0 || state().Ibvj_03 == 1))
                {
                    for(k=0; k<state().Inoj3; k++) if(state().Zbvj03[k].norm() > 0.0)
                    {
                        ifvcj = 1;
                        break;
                    }
                }
                
                
                for(k=0; k<nPoSphe; k++)
                { checkpoint(); 
                    TMele[k].set(0.0, 0.0);
                    TVele[k].set(0.0, 0.0);
                }
                
                if(state().Ifadmj3) {state().Admia3 = state().zbval1[nelj];} else {state().Admia3.set(0.0, 0.0);}
                NC_ComputeEntriesTforFMM(NCout, nelj, ifvcj, state().clulevarry[nlv].nPoinSpheLv, nfmgp,
                              cencli, state().clulevarry[nlv].uvcsphe, TMele, TVele);
                
                
                if(runtime->compactFar)packed_store_t(nlv,i,j,TMele);
                jdifdar = j*ndofi;
                if(rfa_cl == -1) {dfa_cl = -1.0;} else {dfa_cl = 1.0;}
                
                for(k=0; k<nPoSphe; k++)
                { checkpoint(); 
                    iadrek = ientry + k*ncldofi + jdifdar;
                    
                    if(!runtime->compactFar)zTmxLn[iadrek] = TMele[k]*dfa_cl;
                    if(idfs == 2)  zTmxLn[++iadrek] = TVele[k]*dfa_cl;
                    
                    if(ifvcj) zTvcLn[irownu + k] += TVele[k];
                }
            } 
            irownu += nPoSphe;
            ientry += nPoSphe*ncldofi;
        } 
        
        if(!runtime->compactFar && irowTmxLn[nrowT] != iadrek + 1)
        {
            NC_Error_Exit_2(NCout, "iadrek + 1 must be equal to irowTmxLn[nrowT]!",
                            "irowTmxLn[nrowT] = ", irowTmxLn[nrowT],
                            "iadrek = ", iadrek);
        }
    } 
}


void NC_BuildDforMLFMM
(
	ofstream& NCout
 )
{
    int i, j, jcl, k, jgp, identry, ikjadre;
    double dij, dkij, scprd;
    Complex zmun, zi;
    Vector<Complex> zH1m(state().clulevarry[0].nExpaTermLv), zdiat(state().clulevarry[0].nPoinSpheLv);
    Vector<double> Pm(state().clulevarry[0].nExpaTermLv), disij(NDIM);
    Complex *zwork_d;
    int expalength, npointsphere;
    expalength = state().clulevarry[0].nExpaTermLv;
    npointsphere = state().clulevarry[0].nPoinSpheLv;
    
    for(int nlv=0; nlv<state().numClusterLevels_; nlv++)
    { checkpoint(); 
        
        zwork_d = state().dmtxlev[nlv].zDmxLv;
        
        
        identry = 0; 
        for(i=0; i<state().clulevarry[nlv].nClustOLv; i++) { checkpoint(); 
	  
	  for(j=0; j<state().clulevarry[nlv].ClastArLv[i].NumFanClus; j++) { checkpoint(); 
	    jcl = state().clulevarry[nlv].ClastArLv[i].NumsFanClus[j];
            if(runtime->compactFar && jcl<i)continue;
	    for(k=0; k<NDIM; k++)
	      disij[k] = state().clulevarry[nlv].ClastArLv[i].CoorCent[k] - state().clulevarry[nlv].ClastArLv[jcl].CoorCent[k];
	    Vcnorm_dim3_(disij, dij);
	    dkij = dij*state().waveNumbers_;

	    if(state().adapt_fmmlength_ || runtime->compactFar) {
	      
	      
	      
	      double d = state().clulevarry[nlv].ClastArLv[i].RadiClus + state().clulevarry[nlv].ClastArLv[jcl].RadiClus;
	      double rw = d*state().waveNumbers_ + 1.8*log10(d*state().waveNumbers_ + PI);
	      expalength = (int)(rw);
	      if((double)expalength - rw >= 0.5) expalength++;
	      if(expalength < state().minExpansionTermsFMM_) 
		expalength = state().minExpansionTermsFMM_;
	      if(runtime->compactFar)expalength=std::min(state().clulevarry[nlv].nExpaTermLv,std::max(state().minExpansionTermsFMM_,int(std::floor(rw+.5))+5));
              NC_SphericalHankel(NCout, zH1m, expalength,dkij);
	    }
	    else {
	      
	      NC_SphericalHankel(NCout, zH1m, state().clulevarry[nlv].nExpaTermLv, dkij);
	      expalength = state().clulevarry[nlv].nExpaTermLv;
	    }
	    for(jgp=0; jgp<state().clulevarry[nlv].nPoinSpheLv; jgp++)
	      { checkpoint(); 
		scprd = Scprod_dim3_(disij, state().clulevarry[nlv].uvcsphe[jgp]);
                
		
		NC_LegendrePolynomes(NCout, Pm, state().clulevarry[nlv].nExpaTermLv, scprd);
		
		
		zmun.set(0.0, 0.0);
		zi.set(1.0, 0.0);
		for(k = 0; k < expalength; k++)
		  { checkpoint(); 
		    zmun += zi*zH1m[k]*Pm[k]*(double)(2*k + 1);
		    zi.mul_i(state().harmonicTimeFactor_); 
		  }
		zdiat[jgp] = zmun;
	      } 
                
                
                for(k=0; k<state().clulevarry[nlv].nPoinSpheLv; k++)
                { checkpoint(); 
                    if(runtime->compactFar){packed_store_d(nlv,i,j,zdiat);break;}
                    ikjadre = identry + k*state().clulevarry[nlv].ClastArLv[i].NumFanClus + j;
                    zwork_d[ikjadre] = zdiat[k];
                }
            } 
            identry += state().clulevarry[nlv].nPoinSpheLv*state().clulevarry[nlv].ClastArLv[i].NumFanClus;
        } 
    } 
}


void NC_BuildSforMLFMM
(
	ofstream& NCout
)
{
	int icl, iel, nel, i, jip, iterm0, nlv;
	double scpr_dv, d0, wavruim = state().waveNumbers_*state().harmonicTimeFactor_;
	Complex z0;
	Vector<double> dcenel(NDIM);
	bool ifadm;
	Vector<Complex> zvcun(state().clulevarry[0].nPoinSpheLv), zvcdun(state().clulevarry[0].nPoinSpheLv);
	double *norel;

	int nPoSphe;
	ElCluster *clustArraLn;
	Complex *zSmxLn;
	int *irowSmxLn;
	double **uvshln;
	double *wshln;

	
	for(nlv=0; nlv<state().numClusterLevels_; nlv++)
	{ checkpoint(); 
		
		nPoSphe = state().clulevarry[nlv].nPoinSpheLv;
		clustArraLn = state().clulevarry[nlv].ClastArLv;
		zSmxLn = state().smtxlev[nlv].zSmxLv;
		irowSmxLn = state().smtxlev[nlv].irowSmxLv;
		uvshln = state().clulevarry[nlv].uvcsphe;
		wshln = state().clulevarry[nlv].weisphe;

		
		for(icl=0; icl<state().clulevarry[nlv].nClustOLv; icl++)
		{ checkpoint(); 
			ifadm = clustArraLn[icl].IfAdmiBc;

			
			for(iel=0; iel<clustArraLn[icl].NumOfEl; iel++)
			{ checkpoint(); 
				nel = clustArraLn[icl].NumsOfEl[iel]; 

				
				for(i=0; i<NDIM; i++) dcenel[i] = state().centel[nel][i] - clustArraLn[icl].CoorCent[i];

				
				norel = state().elenor[nel];

				
				for(jip=0; jip<nPoSphe; jip++)
				{ checkpoint(); 
					
					scpr_dv = Scprod_dim3_(dcenel, uvshln[jip]);

					
					d0 = scpr_dv*wavruim;
					z0.set(cos(d0), sin(d0));
					z0 *= wshln[jip];
					zvcun[jip] = z0;

					d0 = Scprod_dim3_(uvshln[jip], norel);
					d0 *= wavruim;
					z0.mul_i(d0);
					zvcdun[jip] = z0;
				} 

				
				iterm0 = irowSmxLn[state().jelist[nel][0]];

                for(i=0; i<nPoSphe; i++)
                { checkpoint(); zSmxLn[iterm0 + i] = zvcun[i]*state().Gama3 + zvcdun[i]*(state().zBta3*state().Tao_);}

			} 
		} 
	} 
}


void NC_BuildNforMLFMM
(
	ofstream& NCout
 )
{
    int i, ic, ie, iel, k, k1, nori_clu;
    Vector<bool> bneacluso(state().numOriginalClusters_);
    Vector<int> iadreclu(state().numOriginalClusters_);
    
    
    for(i=0; i<state().irownea[state().numRowsOfCoefficientMatrix_]; i++) state().zcoefl[i].set(0.0, 0.0);
    
    
    for(ic=0; ic<state().clulevarry[state().nlevtop_].nClustOLv; ic++)
    { checkpoint(); 
        
        bneacluso = false;
        for(k=0; k<state().ClustArray[ic].NumNeaClus; k++)
        { checkpoint(); 
            nori_clu = state().ClustArray[state().ClustArray[ic].NumsNeaClus[k]].OriClust;
            if(!bneacluso[nori_clu]) bneacluso[nori_clu] = true;
        }
        
        
        iadreclu = -1;
        k1 = 0;
        for(k=0; k<state().numOriginalClusters_; k++)
        { checkpoint(); 
            if(!bneacluso[k]) continue;
            iadreclu[k] = k1;
            k1 += state().ClustArray[k].NumOfDOFs;
        }
        
        
        for(ie=0; ie<state().ClustArray[ic].NumOfEl; ie++)
        { checkpoint(); 
            iel = state().ClustArray[ic].NumsOfEl[ie]; 
            
            
            for(i=0; i<NDIM; i++)
            { checkpoint(); 
                state().Sourpoi3[i] = state().centel[iel][i];
                state().Norvci3[i] = state().elenor[iel][i];
            }
            
            
            state().Souradr3[0] = state().jelist[iel][0];
            state().Souradr3[1] = state().jelist[iel][1];
            
            
            switch(state().ibval[iel])
            {
                case 0:         
                    state().Ibvi03 = 0;
                    state().Ifadmii_3 = 0;
                    break;
                case 1:         
                    state().Ibvi03 = 1;
                    state().Ifadmii_3 = 0;
                    break;
                case 2:         
                    state().Ibvi03 = 0;
                    state().Ifadmii_3 = 1;
                    break;
                case 3:         
                    state().Ibvi03 = 2;
                    state().Ifadmii_3 = 0;
                    break;
                case 4:         
                    state().Ibvi03 = 2;
                    state().Ifadmii_3 = 1;
                    break;
            }
            state().Zbvi03 = 0.0;
            for(i=0; i<state().listNumberNodesPerElement[iel]; i++) state().Zbvi03 += state().zbval0[iel][i];
            state().Zbvi03 /= (double)state().listNumberNodesPerElement[iel];
            
            
            if(state().listElementProperty[iel] == 0) 
            {
                if(state().Ifadmii_3) {state().Admia3 = state().zbval1[iel];} else {state().Admia3.set(0.0, 0.0);}
                NC_ComputeEntriesNforFMM(NCout, ic, iel, iadreclu);
            }
        } 
    } 
}


void NC_RHSvecTofSLFMM
(
	ofstream& NCout
)
{
	int i, j;
	Vector<Complex> zvc_d(state().dmtxlev[0].nRowsD);

	
	for(i=0; i<state().dmtxlev[0].nRowsD; i++)
	{ checkpoint(); 
		zvc_d[i].set(0.0, 0.0);
		for(j=state().dmtxlev[0].irowDmxLv[i]; j<state().dmtxlev[0].irowDmxLv[i + 1]; j++)
			zvc_d[i] += state().dmtxlev[0].zDmxLv[j]*state().ztvct[(state().clustarry[i/state().numIntegrationPointsUnitSphere_].NumsFarClus[j-state().dmtxlev[0].irowDmxLv[i]]*state().numIntegrationPointsUnitSphere_+i%state().numIntegrationPointsUnitSphere_)];
	}

	
	for(i=0; i<state().numRowsOfCoefficientMatrix_; i++)
	{ checkpoint(); 	
		for(j=state().irowsmtx[i]; j<state().irowsmtx[i + 1]; j++)
				state().zrhs[i] += state().zsmtx[j]*zvc_d[(j+state().jcolsmtx[i])];
	}
}


void NC_RHSvecTofMLFMM
(
	ofstream& NCout
)
{
	int i, j, ilv;
	if(runtime->compactFar){packed_apply(nullptr,nullptr);return;}
	Vector<Complex> zvc_d(state().maxRowNumberD_);

	
	for(ilv=0; ilv<state().numClusterLevels_; ilv++)
	{ checkpoint(); 
		for(i=0; i<state().dmtxlev[ilv].nRowsD; i++)
		{ checkpoint(); 
			state().clulevarry[ilv].zwkS[i].set(0.0, 0.0);
			for(j=state().dmtxlev[ilv].irowDmxLv[i]; j<state().dmtxlev[ilv].irowDmxLv[i+1]; j++)
				state().clulevarry[ilv].zwkS[i] += 
				state().dmtxlev[ilv].zDmxLv[j]*state().tmtxlev[ilv].zTvcLv[state().dmtxlev[ilv].jcolDmxLv[j]];
		}

		for(i=0; i<state().numRowsOfCoefficientMatrix_; i++)
		{ checkpoint(); 	
			for(j=state().smtxlev[ilv].irowSmxLv[i]; j<state().smtxlev[ilv].irowSmxLv[i+1]; j++) 
				state().zrhs[i] += state().smtxlev[ilv].zSmxLv[j]*state().clulevarry[ilv].zwkS[state().smtxlev[ilv].jcolSmxLv[j]];
		}
	} 
}



void NC_ComputeEntriesTforFMM
(
	ofstream& NCout,
	const int& nelj,	    
	const int& ifvcj, 		
	int& nptshe,			
	int& nfmgp,
	double* cencli,			
	double** uvsph,			
	Vector<Complex>& TMele, 
	Vector<Complex>& TVele  
)
{
    int i, inodj, igp, jgp;
    double vljaco, wga, dw1, wavruim = state().waveNumbers_*state().harmonicTimeFactor_;
    Vector<double> shfunx(NNODPE), shnds(NNODPE), shndt(NNODPE), dxds(NDIM),
    dxdt(NDIM), crdpoi(NDIM), elnorv(NDIM), vwk1(NDIM);
    Complex zbgao_0, zw1, zresu, zrev;
    
    
    inodj = state().listNumberNodesPerElement[nelj]; 
    
    
    for(igp=0; igp<nfmgp; igp++)
    { checkpoint(); 
        
        
        vljaco = NC_ComputeParameters(shfunx, shnds, shndt, dxds, dxdt,
                                      crdpoi, elnorv, zbgao_0, state().Crdej3, ifvcj,
                                      state().Zbvj03, inodj, state().csiFMgau[igp], state().etaFMgau[igp]);
        wga = state().weiFMgau[igp]*vljaco;
        
        
        for(jgp=0; jgp<nptshe; jgp++)
        { checkpoint(); 
            for(i=0; i<NDIM; i++)
            { checkpoint(); 
                vwk1[i] = cencli[i] - crdpoi[i];
            }
            dw1 = (Scprod_dim3_(vwk1, uvsph[jgp]))*wavruim;
            zresu.set(cos(dw1), sin(dw1));
            if(ifvcj)
            {
                if(state().Ibvj_03 == 0)  {
                    zrev = zresu;
                    zrev *= (zbgao_0*wga);
                    TVele[jgp] += zrev;
                }
            }
            if(state().Ibvj_03 == 0) {
                dw1 = (Scprod_dim3_(elnorv, uvsph[jgp]))*state().waveNumbers_*state().Tao_;
                if(state().Admia3.norm() == 0.0) zresu *= (dw1*wga);
                else
                {
                    zw1.set(dw1 - state().harmonicTimeFactor_*state().Admia3.im(), state().harmonicTimeFactor_*state().Admia3.re());
                    zresu *= (zw1*wga);
                }
		
            }
            if(state().Ibvj_03 == 1) {  
                zresu *= wga;
                
		zresu.mul_i(-1.0);

		
                if(ifvcj) {
                    dw1 = (Scprod_dim3_(elnorv, uvsph[jgp]))*state().waveNumbers_*state().Tao_;
                    TVele[jgp] -= zresu*zbgao_0*dw1;
                }
            }
            TMele[jgp] += zresu;
        } 
    } 
    
    dw1 = state().waveNumbers_/PI4;
    for(i=0; i<nptshe; i++) TMele[i] *= dw1;
    dw1 *= state().Tao_*state().harmonicTimeFactor_;
    if(ifvcj) for(i=0; i<nptshe; i++) TVele[i].mul_i(dw1);
}


void NC_ComputeEntriesNforFMM
(
	ofstream& NCout,
	const int& ic,           
	const int& iel,          
	Vector<int>& iadreclu    
)
{
    int jel, i, j, j1;
    Complex z0, localAdmittance(0.0, 0.0);
    
    int nu_cl, nrf_cl, rfa_cl, nuo_cl, jadr0_cl, jadrf_cl, jadre_cl;
    bool mir_cl;
    Vector<bool> rfdi_cl(NDIM);
    
    
    NC_SearchAddressNforFMM(NCout, j1, state().Souradr3[0], state().Souradr3[0], 'o');
    switch(state().Ibvi03)
    {
        case 0:
	  
            state().zcoefl[j1] += (state().Admia3*state().zBta3 - state().Gama3)*0.5;
            break;
        case 1:
            state().zcoefl[j1] -= state().zBta3*(0.5*state().Tao_);
            break;
        default:
            NC_Error_Exit_2(NCout, "state().Ibvi03 must be equal to 0 or 1!", "state().Ibvi03 = ", state().Ibvi03,
                            "element number = ", state().extNumbersOfElements[iel]);
    }
    
    
    z0 = 0.0;
    
    if(state().Ibvi03 == 0) 
    {
        z0 += state().Zbvi03*(state().zBta3*state().Tao_*0.5);
    }
    if(state().Ibvi03 == 1) 
    {
        z0 += state().Zbvi03*state().Tao_*0.5;
    }
    
    if(state().numIncidentPlaneWaves_ + state().numPointSources_ > 0)
    {
        z0 += NC_IncidentWaveRHS(NCout);
    }
    state().zrhs[state().Souradr3[0]] += z0;
    
    
    for(j=0; j<state().ClustArray[ic].NumNeaClus; j++)
    { checkpoint(); 
        
        nu_cl = state().ClustArray[ic].NumsNeaClus[j];
        nuo_cl = state().ClustArray[nu_cl].OriClust;
        jadr0_cl = iadreclu[nuo_cl];
        jadrf_cl = state().ClustArray[nu_cl].NDOFsPeEl;
        nrf_cl = state().ClustArray[nu_cl].nuref;
        rfa_cl = state().ClustArray[nu_cl].rffac;
        if(nrf_cl)
        {
            mir_cl = state().ClustArray[nu_cl].ifmirro;
            for(i=0; i<NDIM; i++) rfdi_cl[i] = state().ClustArray[nu_cl].ifrfdi[i];
        }
        
        
        for(j1=0; j1<state().ClustArray[nu_cl].NumOfEl; j1++)
        { checkpoint(); 
            
            jel = state().ClustArray[nu_cl].NumsOfEl[j1];
            
            
            NC_ComputeElementData(jel, 0, state().Inoj3, state().Ibvj_03,
                                  state().Ifadmj3, state().Jdofaddr3, state().Centej3, state().Norvcj3, state().Admj3,
                                  state().Zbvj03, state().Crdej3);
            
            
            state().Ifcrh3 = 0;
            if((state().Ibvj_03 == 0 || state().Ibvj_03 == 1) && state().listElementProperty[jel] == 0) for(i=0; i<state().Inoj3; i++)
                if(state().Zbvj03[i].norm() > EPSY) {state().Ifcrh3 = 1; break;}
            
            
            if(nrf_cl) NC_ReflectElementFMM(rfa_cl, mir_cl, rfdi_cl, state().Inoj3, state().Ibvj_03,
                                            state().Centej3, state().Norvcj3, state().Zbvj03, state().Crdej3);
            
            
            if(nrf_cl == 0 && jel == iel) 
            {
                NC_SingularIntegration(NCout, state().Zrsintel, state().Inoj3, state().Ifcrh3, state().Zbvj03,
                                       state().Crdej3);
            }
            else 
            {
                NC_RegularIntegration(NCout, state().Zrsintel, iel, jel, state().Inoj3,
                                      state().Ifcrh3, state().Zbvj03, state().Centej3, state().Crdej3);
            }
            
            
            jadre_cl = jadr0_cl + j1*jadrf_cl;
            NC_AssembleNforFMM(NCout, jel, jadre_cl, state().Souradr3, rfa_cl);
        } 
    } 
}


void NC_AssembleNforFMM
(
	ofstream& NCout,
	const int& jel,			 
	const int& jadre_cl,     
	Vector<int>& arg_Souradr3,	 
	int& rfa_cl             
)
{
    Complex zsum0, zsum1;
    int iadr, kdadr;
    
    kdadr = jadre_cl;
    
    if(state().Ibvj_03 == 0) { 
        zsum0 = state().Zrsintel[1]*(state().Gama3*state().Tao_) + state().Zrsintel[3]*state().zBta3;
        if(state().Ifadmj3)
            zsum0 += (state().Zrsintel[0]*state().Gama3 + state().Zrsintel[2]*(state().zBta3*state().Tao_))*state().Admj3[0];
    }
    if(state().Ibvj_03 == 1) { 
        zsum0 = state().Zrsintel[0]*(-state().Gama3*state().Tao_) - state().Zrsintel[2]*(state().zBta3);
        if(state().Ifadmj3) {
            fprintf(stderr,"Sorry not implemented yet\n");
            fail_exit(-1);
        }
    }
    
    iadr = state().irownea[arg_Souradr3[0]] + kdadr;
    if(rfa_cl == -1) state().zcoefl[iadr] -= zsum0;
    else state().zcoefl[iadr] += zsum0;
    
    if(state().Ifcrh3) state().zrhs[arg_Souradr3[0]] += state().Zrsintel[4];
}


void NC_SearchAddressNforFMM
(
	ofstream& NCout,
	int& iadr,         
	int& nrow,         
	int& ncol,         
	const char& c	   
)
{
    int i;
    string strout("Address of an entry not found _ ");
    
    strout += c;
    
    iadr = -1;
    for(i=state().irownea[nrow]; i<state().irownea[nrow + 1]; i++) if(state().jcolnea[i+state().nearColumnBias[nrow]] == ncol)
    {
        iadr = i;
        break;
    }
    if(iadr == -1) NC_Error_Exit_2(NCout, strout, "Row number = ", nrow,
                                   "Column number = ", ncol);
}


void NC_SingularIntegration
(
	ofstream& NCout,
	Vector<Complex>& zrsintel, 
 
 
 
 
 
	const int& inodj,          
	const int& ifcrhs,         
	Vector<Complex>& zbvj_0,   
	Matrix<double>& crdelj    
)
{
    int ieg, i, ig1, ngpoi, ig, ngpo1 = 3, nsec1 = 4, nsec2 = 2, ig2, isec, ngausin = 4;
    double  sga, tga, wga, leneg, disfsp, re1, re2, sgg, tgg, vljaco, wnu2, secmid,
    delsec, wavruim= state().harmonicTimeFactor_*state().waveNumbers_;
    Vector<double> crdgp(NDIM), difpoi(NDIM), difpoo(NDIM), diffsp(NDIM);
    Complex zg, zhh, zht, z1, zre, zbgao_0, zbv;
    Vector<Complex> zdgdy(NDIM), zwk(NDIM);
    double ssub[3], tsub[3], aresub = 0.0, csigau[36], etagau[36], weigau[36];
    Vector<double> shfunx(NNODPE), shnds(NNODPE), shndt(NNODPE), dxds(NDIM),
    dxdt(NDIM), crdpoi(NDIM), elnorv(NDIM), coordgau(ngpo1), weitgau(ngpo1);
    
    
    for(i=0; i<6; i++) zrsintel[i].set(0.0, 0.0);
    wnu2 = state().waveNumbers_*state().waveNumbers_;
    
    
    BLGauCooWe(NCout, ngpo1, coordgau, weitgau);
    
    
    for(ieg=0; ieg<inodj; ieg++)
    { checkpoint(); 
        
        ig1 = ieg + 1;
        if(ig1 == inodj) ig1 = 0;
        ig2 = ieg + inodj;
        
        
        leneg = 0.0;
        for(i=0; i<NDIM; i++)
        { checkpoint(); 
            difpoi[i] = crdelj(ig1, i) - crdelj(ieg, i);
            leneg += difpoi[i]*difpoi[i];
        }
        leneg = sqrt(leneg); 
        for(i=0; i<NDIM; i++) difpoo[i] = difpoi[i]/leneg; 
        leneg /= (2.0*(double)nsec1);
        
        zre.set(0.0, 0.0);
        
        
        delsec = 2.0/(double)nsec1;
        secmid = -1.0 - delsec/2.0;
        for(isec=0; isec<nsec1; isec++) { checkpoint(); 
            secmid += delsec;
            
            
            for(ig=0; ig<ngpo1; ig++)
            { checkpoint(); 
                sga = secmid + coordgau[ig]/(double)nsec1;
                wga = weitgau[ig]*leneg;
                for(i=0; i<NDIM; i++)
                { checkpoint(); 
                    crdgp[i] = crdelj(ieg, i) + difpoi[i]*(sga + 1.0)/2.0;
                    diffsp[i] = crdgp[i] - state().Sourpoi3[i];
                }
		
                Vcnorm_dim3_(diffsp, disfsp); 
                re1 = wavruim*disfsp;
                re2 = PI4*disfsp;
                zg.set(cos(re1)/re2, sin(re1)/re2);
                
                z1.set(-1.0/disfsp, wavruim);
                zg.mul_c(z1);
                for(i=0; i<NDIM; i++) zdgdy[i] = zg*diffsp[i];
                zwk[0] = zdgdy[1]*difpoo[2] - zdgdy[2]*difpoo[1];
                zwk[1] = zdgdy[2]*difpoo[0] - zdgdy[0]*difpoo[2];
                zwk[2] = zdgdy[0]*difpoo[1] - zdgdy[1]*difpoo[0];
                zre += (zwk[0]*state().Norvci3[0] + zwk[1]*state().Norvci3[1] + zwk[2]*state().Norvci3[2])*wga;
                
            } 
        } 
        zrsintel[3] += zre;
        
        
        for(isec=0; isec<nsec2; isec++) { checkpoint(); 
            
            
            switch(inodj)
            {
	      
                case 3: 
                    aresub = 1.0/24.0/(double)nsec2;
                    ssub[0] = tsub[0] = 1.0/3.0;
                    if(isec == 0) {
		      
                        ssub[1] = CSI6[ieg];
                        ssub[2] = CSI6[ig2];
                        tsub[1] = ETA6[ieg];
                        tsub[2] = ETA6[ig2];
                    }
                    else
                    {
                        ssub[1] = CSI6[ig2];
                        ssub[2] = CSI6[ig1];
                        tsub[1] = ETA6[ig2];
                        tsub[2] = ETA6[ig1];
                    }
                    break;
                case 4: 
                    aresub = 0.25/(double)nsec2;
                    ssub[0] = tsub[0] = 0.0;
                    if(isec == 0) {
                        ssub[1] = CSI8[ieg];
                        ssub[2] = CSI8[ig2];
                        tsub[1] = ETA8[ieg];
                        tsub[2] = ETA8[ig2];
                    }
                    else
                    {
                        ssub[1] = CSI8[ig2];
                        ssub[2] = CSI8[ig1];
                        tsub[1] = ETA8[ig2];
                        tsub[2] = ETA8[ig1];
                    }
                    break;
                default:
                    NC_Error_Exit_1(NCout, "inodj must be 3 or 4!", "inodj = ", inodj);
            }
            
            
            ngpoi = NC_ComputeGausseanPoints(csigau, etagau, weigau, 4, ngausin);
            
            
            for(ig=0; ig<ngpoi; ig++)
            { checkpoint(); 
                sga = csigau[ig];
                tga = etagau[ig];
                
                
                sgg = 0.5*(1.0 - sga)*ssub[0] +
                0.25*(1.0 + sga)*((1.0 - tga)*ssub[1] + (1.0 + tga)*ssub[2]);
                tgg = 0.5*(1.0 - sga)*tsub[0] +
                0.25*(1.0 + sga)*((1.0 - tga)*tsub[1] + (1.0 + tga)*tsub[2]);
                
                
                vljaco = NC_ComputeParameters(shfunx, shnds, shndt, dxds, dxdt,
                                              crdpoi, elnorv, zbgao_0, crdelj, ifcrhs,
                                              zbvj_0, inodj, sgg, tgg);
                
                wga = weigau[ig]*(1.0 + sga)*aresub*vljaco;
                
                
                for(i=0; i<NDIM; i++) diffsp[i] = crdpoi[i] - state().Sourpoi3[i];
                Vcnorm_dim3_(diffsp, disfsp); 
                
                re1 = wavruim*disfsp;
                re2 = wga/PI4/disfsp;
                zg.set(cos(re1)*re2, sin(re1)*re2);
                
                z1.set(-1.0/disfsp, wavruim);
                zhh = zht = zg*z1;
                re1 = diffsp*elnorv;
                re2 = -(diffsp*state().Norvci3);
                zhh.mul_r(re1);
                zht.mul_r(re2);
                
                zrsintel[0] += zg;
                zrsintel[1] += zhh;
                zrsintel[2] += zht;
                
                zrsintel[3] += zg*(wnu2*Scprod_dim3_(state().Norvci3, elnorv));
                
                if(ifcrhs && state().Ibvj_03==0)
                {
                    zrsintel[4] += (zg*(state().Gama3*state().Tao_) + zht*state().zBta3)*zbgao_0;
                }
            } 
        } 
    } 
    
    if(ifcrhs && state().Ibvj_03==1)
    {
        zrsintel[4] = ( zrsintel[1]*(state().Gama3*state().Tao_) + zrsintel[3]*state().zBta3 )*zbgao_0;
        zrsintel[4] *= -1;
    }
}


void NC_RegularIntegration
(
	ofstream& NCout,
	Vector<Complex>& zrsintel, 
 
 
 
 
 
	const int& iel,            
	const int& jel,            
	const int& inodj,          
	const int& ifcrhs,         
	Vector<Complex>& zbvj_0,   
	Vector<double>& centerj,   
	Matrix<double>& crdelj    
)
{
    int i, ngpoi, jgp;
    double vljaco, wga, disfsp, re1, re2, rq, dq, wavruim = state().harmonicTimeFactor_*state().waveNumbers_;
    double csigau[36], etagau[36], weigau[36];
    Vector<double> diffsp(NDIM);
    Complex zg, zhh, zht, ze, z1, zbgao_0, zbv;
    Vector<double> shfunx(NNODPE), shnds(NNODPE), shndt(NNODPE), dxds(NDIM),
    dxdt(NDIM), crdpoi(NDIM), elnorv(NDIM);
    
    
    int nsbe, jsb, iforie;
    double fase, fase2, xio, eto, weih2, xice, etce;
    Vector<double> xisbp(MSBE), etsbp(MSBE), fasbp(MSBE), csip(NNODPE), etap(NNODPE);
    Vector<int> ngsbp(MSBE);
    
    
    for(i=0; i<6; i++) zrsintel[i].set(0.0, 0.0);
    
    
    if(inodj == NETYP3)	for(i=0; i<inodj; i++)
    { checkpoint(); 
        csip[i] = CSI6[i];
        etap[i] = ETA6[i];
    }
    else for(i=0; i<inodj; i++)
    { checkpoint(); 
        csip[i] = CSI8[i];
        etap[i] = ETA8[i];
    }
    
    
    nsbe = NC_GenerateSubelements(NCout, -1, state().Sourpoi3, jel, crdelj,
                                  state().areael[jel], inodj, csip, etap, inodj,
                                  xisbp, etsbp, fasbp, ngsbp); 
    
    
    
    for(jsb=0; jsb<nsbe; jsb++)
    { checkpoint(); 
        
        xice = xisbp[jsb]; 
        etce = etsbp[jsb]; 
        fase = fasbp[jsb]; 
        fase2 = fase*fase; 
        
        if(fabs(fase - 1.0) < EPSY) iforie = 1; 
        else iforie = 0; 
        
        
        ngpoi = NC_ComputeGausseanPoints(csigau, etagau, weigau, inodj, ngsbp[jsb]); 
        
        
        for(jgp=0; jgp<ngpoi; jgp++)
        { checkpoint(); 
            
            if(iforie)
            {
                xio = csigau[jgp];
                eto = etagau[jgp];
                weih2 = weigau[jgp];
            }
            else
            {
                xio = xice + csigau[jgp]*fase;
                eto = etce + etagau[jgp]*fase;
                weih2 = weigau[jgp]*fase2;
            }
            
            
            vljaco = NC_ComputeParameters(shfunx, shnds, shndt, dxds, dxdt,
                                          crdpoi, elnorv, zbgao_0, crdelj, ifcrhs,
                                          zbvj_0, inodj, xio, eto);
            wga = weih2*vljaco;
            
            
            
            for(i=0; i<NDIM; i++) diffsp[i] = crdpoi[i] - state().Sourpoi3[i];
            Vcnorm_dim3_(diffsp, disfsp); 
            re1 = wavruim*disfsp;
            re2 = wga/PI4/disfsp;
            zg.set(cos(re1)*re2, sin(re1)*re2);
            
            
            z1.set(-1.0/disfsp, wavruim);
            zhh = zht = zg*z1;
            re1 = diffsp*elnorv;
            re2 = -(diffsp*state().Norvci3);
            zhh.mul_r(re1);
            zht.mul_r(re2);
            
            
            rq = re1*re2;
            re1 = state().Norvci3*elnorv; 
            dq = disfsp*disfsp;
            z1.set((3.0/dq - state().waveNumbers_*state().waveNumbers_)*rq + re1/dq, -wavruim/disfsp*(3.0*rq + re1));
            ze = zg*z1;
            
            zrsintel[0] += zg;
            zrsintel[1] += zhh;
            zrsintel[2] += zht;
            zrsintel[3] += ze;
            
            if(ifcrhs) 
            {
                if(state().Ibvj_03 == 0)  
                    zrsintel[4] += (zg*(state().Gama3*state().Tao_) + zht*state().zBta3)*zbgao_0;
                if(state().Ibvj_03 == 1)  
                    zrsintel[4] -= (zhh*(state().Gama3*state().Tao_) + ze*state().zBta3)*zbgao_0;
            } 
        } 
    } 
}


Complex NC_IncidentWaveRHS
(
	ofstream& NCout
)
{
    int i, j, kref;
    double re1, reffac, wavruim = state().harmonicTimeFactor_*state().waveNumbers_;
    Complex zrc(0.0, 0.0), zg, zv, zini;
    Vector<double> vect1(NDIM), vect2(NDIM);
    
    
    if(state().numIncidentPlaneWaves_ > 0)
    {
        Vector<double> sourposym(state().Sourpoi3, NDIM);
        if(state().numSymmetricPlanes_ > 0)
        {
            for(i=0; i<NDIM; i++) if(state().listSymmetryPlanes[i] != 0) sourposym[i] -= state().listSymmetryPlanesCoordinates[i];
        }
        
        
        for(i=0; i<state().numIncidentPlaneWaves_; i++)
        { checkpoint(); 
            
            for(j=0; j<NDIM; j++) vect1[j] = state().dirinw[i][j];
            
            
            zini = state().zinwav[i];
            reffac = 1.0;
            for(kref=0; kref<state().numReflectionsOfElements_; kref++)
            { checkpoint(); 
                if(kref > 0) reffac = NC_ReflectPointVector(vect1, kref, reffac, 0);
                
                re1 = vect1*sourposym*wavruim;
                zg.set(cos(re1)*reffac, sin(re1)*reffac);
                zg.mul_c(zini);
                
                zv = zg;
                re1 = wavruim*(vect1*state().Norvci3);
                zv.mul_i(re1);
                
                zrc -= zg*state().Gama3 + zv*(state().zBta3*state().Tao_);
            }
        } 
    }
    
    
    if(state().numPointSources_ > 0)
    {
        double aglen, re2;
        
        
        for(i=0; i<state().numPointSources_; i++)
        { checkpoint(); 
            
            for(j=0; j<NDIM; j++) vect1[j] = state().coorps[i][j];
            
            
            zini = state().zposoc[i];
            reffac = 1.0;
            for(kref=0; kref<state().numReflectionsOfElements_; kref++)
            { checkpoint(); 
                if(kref > 0) reffac = NC_ReflectPointVector(vect1, kref, reffac, 1);
                
                for(j=0; j<NDIM; j++) vect2[j] = state().Sourpoi3[j] - vect1[j];
                Vcnorm_dim3_(vect2, aglen);
                
                re1 = wavruim*aglen;
                re2 = reffac/PI4/aglen;
                zg.set(cos(re1)*re2, sin(re1)*re2);
                zg.mul_c(zini);
                
                re1 = vect2*state().Norvci3;
                zv.set(-re1/aglen, wavruim*re1);
                zv *= zg;
                
                zrc -= zg*state().Gama3 + zv*(state().zBta3*state().Tao_);
            }
        } 
    }
    
    return zrc;
}


void NC_UpdateFreqCurves
(
	ofstream& NCout,
	double* Freqs
 )
{
    int i, j, k, ibg = 0, ied = 0, ibc;
    double facf, gammfact = state().waveNumbers_*state().harmonicTimeFactor_;
    Complex zv;
    
    
    for(i=0; i<state().numElements_; i++)
    { checkpoint(); 
        if(state().listElementProperty[i] == 2) continue;
        
        
        for(j=0; j<state().listNumberNodesPerElement[i]; j++) state().zbval0[i][j] = state().zbvao0[i][j];
        state().zbval1[i] = state().zbvao1[i];
        state().zbval2[i] = state().zbvao2[i];
        
        
        if(state().ibval[i] < 2) continue;
        switch(state().ibval[i])
        {
            case 2:       
                ibg = 2;
                ied = 5;
                break;
            case 3:       
                ibg = 0;
                ied = 1;
                break;
            case 4:       
                ibg = 0;
                ied = 5;
                break;
            default:
                NC_Error_Exit_0(NCout, "state().ibval[i] must be not greater than 4!");
        }
        for(j=ibg; j<=ied; j++)
        { checkpoint(); 
            ibc = state().ibvcurv[i][j];
            if(ibc < 0) continue;
            facf = NC_FreqCurveInterpolation(NCout, ibc);
            switch(j)
            {
                case 0:
                    for(k=0; k<state().listNumberNodesPerElement[i]; k++)
                    { checkpoint(); 
                        state().zbval0[i][k].set(state().zbval0[i][k].re()*facf, state().zbval0[i][k].im());
                    }
                    break;
                case 1:
                    for(k=0; k<state().listNumberNodesPerElement[i]; k++)
                    { checkpoint(); 
                        state().zbval0[i][k].set(state().zbval0[i][k].re(), state().zbval0[i][k].im()*facf);
                    }
                    break;
                case 2:
                    state().zbval1[i].set(state().zbval1[i].re()*facf, state().zbval1[i].im());
                    break;
                case 3:
                    state().zbval1[i].set(state().zbval1[i].re(), state().zbval1[i].im()*facf);
                    break;
                case 4:
                    state().zbval2[i].set(state().zbval2[i].re()*facf, state().zbval2[i].im());
                    break;
                case 5:
                    state().zbval2[i].set(state().zbval2[i].re(), state().zbval2[i].im()*facf);
                    break;
            }
        } 
    } 
    
    
    for(i=0; i<state().numElements_; i++)
    { checkpoint(); 
        if(state().listElementProperty[i] == 2) continue;
        switch(state().ibval[i])
        {
            case 1:  
                for(j=0; j<state().listNumberNodesPerElement[i]; j++)
                { checkpoint(); 
                    state().zbval0[i][j].div_i(state().rpfact_);
                }
                break;
            case 2: 
                state().zbval1[i].mul_i(gammfact);
                break;
            case 3: 
                for(j=0; j<state().listNumberNodesPerElement[i]; j++)
                { checkpoint(); 
                    state().zbval0[i][j].mul_i(gammfact);
                }
                break;
            case 4: 
                for(j=0; j<state().listNumberNodesPerElement[i]; j++)
                { checkpoint(); 
                    state().zbval0[i][j].mul_i(gammfact);
                }
                state().zbval1[i].mul_i(gammfact);
                break;
        }
    } 
    
    
    if(state().numIncidentPlaneWaves_ > 0)
    {
        for(i=0; i<state().numIncidentPlaneWaves_; i++)
        { checkpoint(); 
            zv = state().zinwao[i];
            for(j=0; j<2; j++)
            { checkpoint(); 
                ibc = state().inwacurv[i][j];
                if(ibc >= 0)
                {
                    facf = NC_FreqCurveInterpolation(NCout, ibc);
                    if(j == 0) {zv.set(zv.re()*facf, zv.im());}
                    else {zv.set(zv.re(), zv.im()*facf);}
                }
            }
            zv.div_i(state().rpfact_);
            state().zinwav[i] = zv;
        }
    } 
    
    
    if(state().numPointSources_ > 0)
    {
        for(i=0; i<state().numPointSources_; i++)
        { checkpoint(); 
            zv = state().zposoo[i];
            for(j=0; j<2; j++)
            { checkpoint(); 
                ibc = state().posocurv[i][j];
                if(ibc >= 0)
                {
                    facf = NC_FreqCurveInterpolation(NCout, ibc);
                    if(j == 0) {zv.set(zv.re()*facf, zv.im());}
                    else {zv.set(zv.re(), zv.im()*facf);}
                }
            }
            zv.div_i(state().rpfact_);
            state().zposoc[i] = zv;
        }
    } 
}


double NC_FreqCurveInterpolation
(
	ofstream& NCout, 
	int& nucv
 )
{
    double factor;
    int l, l1 = 0, l2, l3;
    
    
    for(l=0; l<state().numCurvesFrequency_; l++)
    { checkpoint(); 
        if(nucv == state().nucurv[l]) 
        {
            l1 = l;
            goto Interpo_Curve;
        }
    }
    NC_Error_Exit_1(NCout, "A curve can not be found!", "Number of the curve = ", nucv);
    
Interpo_Curve:
    
    if(state().frequency_ <= state().frqcurv[l1][0])
    {
        factor = state().faccurv[l1][0];
        return factor;
    }
    
    for(l=1; l<state().npcurv[l1]; l++)
    { checkpoint(); 
        if(state().frequency_ < state().frqcurv[l1][l])
        {
            l2 = l;
            goto Interpo_Point;
        }
    }
    factor = state().faccurv[l1][state().npcurv[l1] - 1];
    return factor;
    
Interpo_Point:
    l3 = l2 - 1;
    factor = state().faccurv[l1][l3] + (state().faccurv[l1][l2] - state().faccurv[l1][l3])/
    (state().frqcurv[l1][l2] - state().frqcurv[l1][l3])*(state().frequency_ - state().frqcurv[l1][l3]);
    
    return factor;
}
