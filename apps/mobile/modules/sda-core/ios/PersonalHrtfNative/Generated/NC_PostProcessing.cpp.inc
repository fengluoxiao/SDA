









#ifdef isWindows                                                                    

#else                                                                               

#endif                                                                              
using namespace std;                                                                





void NC_ComputeTvecSLFMM(ofstream&);
void NC_ComputeTvecMLFMM(ofstream&);
void NC_ContributionTvecFMM(ofstream&,const int&,int&,Vector<double>&,Matrix<double>&,Complex&,Vector<Complex>&, double**,Vector<Complex>&);
void NC_WriteResultsObjectMesh(ofstream&,Matrix<Complex>&,Matrix<Complex>&,Matrix<Complex>&,Matrix<double>&);
void NC_WriteResultsEvaluationGrid(ofstream&, Matrix<Complex>&, Matrix<Complex>&);
Complex NC_ContributionIncidentWaves(ofstream&,Vector<double>&,Vector<Complex>&,const int&);
void NC_ContributionTBEM(ofstream&,Matrix<Complex>&,Matrix<Complex>&,int&,int&,Vector<double>&,Vector<Complex>&,Matrix<Complex>&);
void NC_ContributionFMM(ofstream&,Matrix<Complex>&,Matrix<Complex>&,Vector<int>&,Vector<Complex>&,Matrix<Complex>&);
Complex NC_IntegrationTBEM(ofstream&,Vector<double>&,const double&,const double&,const int&,const double&,const int&,Matrix<double>,const Complex,const int&,Vector<Complex>&,Vector<Complex>&,const int&, const int&);
void NC_Magnitude2dBdeg(Vector<double>&, const Complex&, const int&);




const double PREREF = 2.0e-5,
			 VELREF = 5.0e-8;














const int GaupFMBOrde = 2;








void NC_PostProcessing
(
	ofstream& NCout
)
{
	Matrix<Complex> cVpotele(state().numElements_, 2), cVeloele(state().numElements_, NNODPE*2), cVeloelo(state().numElements_, NNODPE);
	Matrix<double> rEninele(state().numElements_, NNODPE*2);

	
	sprintf(state().Dfname_3d, "be.out/be.%d",  state().currentFrequency_ + 1);
#ifdef isWindows
	int ifmkd;
    ifmkd = local_mkdir(state().Dfname_3d); 
#else
	local_mkdir(state().Dfname_3d);  
#endif

	
	
#ifdef isWindows
	
#else
	
#endif

	
	if(state().numInternalPointsClusters_) state().IfTvc = true;

	
	if(state().IfTvc)
	{
		switch(state().methodFMM_)
		{
		case 1: 
			state().zT_vc = allocate_array<Complex>((state().numOriginalReflectedClusters_*state().numIntegrationPointsUnitSphere_));
			NC_ComputeTvecSLFMM(NCout);
			break;
		case 3: 
			NC_ComputeTvecMLFMM(NCout);
			break;
		}
	}

	
	NC_WriteResultsObjectMesh(NCout, cVpotele, cVeloele, cVeloelo, rEninele);

	
	if(state().numNodesOfEvaluationMesh_ > 0)
    {
        NC_WriteResultsEvaluationGrid(NCout, cVpotele, cVeloele);
    }

	
	if(state().IfTvc && (state().methodFMM_ == 1 || state().methodFMM_ == 2)) release_array(state().zT_vc);
}


void NC_ComputeTvecSLFMM
(
	ofstream& NCout
 )
{
    int i, j, inod0 = 0, k, ibg, jel, iveloj, ifadmij, inodj;
    Vector<double> center_i(NDIM);
    Vector<int> jdofaddre(2);
    Vector<double> centerj(NDIM), norvecj(NDIM*(NNODPE + 1));
    Matrix<double> crdelj(NNODPE, NDIM);
    Vector<Complex> zpotej(2);
    Vector<Complex> zveloj(NNODPE);
    
    int nrf_cl, rfa_cl;
    bool mir_cl;
    Vector<bool> rfdi_cl(NDIM);
    
    
    
    Vector<Complex> zTele(state().numIntegrationPointsUnitSphere_);
    
    
    for(i=0; i<state().numIntegrationPointsUnitSphere_*state().numOriginalReflectedClusters_; i++) state().zT_vc[i].set(0.0, 0.0);
    ibg = 0;
    for(i=0; i<state().numOriginalReflectedClusters_; i++)
    { checkpoint(); 
        
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
            state().npFMGp = NC_ComputeGausseanPoints(state().csipFMgau, state().etapFMgau, state().weipFMgau, inod0, GaupFMBOrde);
        }
        
        
        for(k=0; k<NDIM; k++) center_i[k] = state().ClustArray[i].CoorCent[k];
        
        
        for(j=0; j<state().ClustArray[i].NumOfEl; j++)
        { checkpoint(); 
            jel = state().ClustArray[i].NumsOfEl[j];
            
            
            if(!state().ClustArray[i].IfMonoEl && state().listNumberNodesPerElement[jel] != inod0)
            {
                inod0 = state().listNumberNodesPerElement[jel];
                state().npFMGp = NC_ComputeGausseanPoints(state().csipFMgau, state().etapFMgau, state().weipFMgau, inod0, GaupFMBOrde);
            }
            
            
            NC_ComputeElementData(jel, 1, inodj, iveloj,
                                  ifadmij, jdofaddre, centerj, norvecj, zpotej,
                                  zveloj, crdelj);
            
            
            if(nrf_cl) NC_ReflectElementFMM(rfa_cl, mir_cl, rfdi_cl, inodj, 0,
                                            centerj, norvecj, zveloj, crdelj);
            if(nrf_cl && rfa_cl == -1) zpotej[0].nega();
            
            
            for(k=0; k<state().numIntegrationPointsUnitSphere_; k++) zTele[k].set(0.0, 0.0);
            NC_ContributionTvecFMM(NCout, jel, state().numIntegrationPointsUnitSphere_, center_i, crdelj, zpotej[0],
                                   zveloj, state().uvcsphe, zTele);
            
            for(k=0; k<state().numIntegrationPointsUnitSphere_; k++) state().zT_vc[ibg + k] += zTele[k];
        } 
        ibg += state().numIntegrationPointsUnitSphere_;
    } 
}


void NC_ComputeTvecMLFMM
(
	ofstream& NCout
 )
{
    int i, j, inod0 = 0, k, ibg, jel, iveloj, ifadmij, inodj, ilv;
    Vector<double> center_i(NDIM);
    Vector<int> jdofaddre(2);
    Vector<double> centerj(NDIM), norvecj(NDIM*(NNODPE + 1));
    Matrix<double> crdelj(NNODPE, NDIM);
    Vector<Complex> zpotej(2);
    Vector<Complex> zveloj(NNODPE);
    
    int nrf_cl, rfa_cl;
    bool mir_cl;
    Vector<bool> rfdi_cl(NDIM);
    
    ElCluster *clusArray;
    
    
    
    Vector<Complex> zTele(state().clulevarry[0].nPoinSpheLv);
    
    
    for(ilv=0; ilv<state().numClusterLevels_; ilv++)
    { checkpoint(); 
        
        clusArray = state().clulevarry[ilv].ClastArLv;
        
        
        for(i=0; i<state().clulevarry[ilv].nPoinSpheLv*state().clulevarry[ilv].nClustSLv; i++)
            state().clulevarry[ilv].zwkT[i].set(0.0, 0.0);
        
        ibg = 0;
        for(i=0; i<state().clulevarry[ilv].nClustSLv; i++)
        { checkpoint(); 
            
            nrf_cl = clusArray[i].nuref;
            rfa_cl = clusArray[i].rffac;
            if(nrf_cl)
            {
                mir_cl = clusArray[i].ifmirro;
                for(j=0; j<NDIM; j++) rfdi_cl[j] = clusArray[i].ifrfdi[j];
            }
            
            
            if(clusArray[i].IfMonoEl)
            {
                inod0 = state().listNumberNodesPerElement[clusArray[i].NumsOfEl[0]];
                state().npFMGp = NC_ComputeGausseanPoints(state().csipFMgau, state().etapFMgau, state().weipFMgau, inod0, GaupFMBOrde);
            }
            
            
            for(k=0; k<NDIM; k++) center_i[k] = clusArray[i].CoorCent[k];
            
            
            for(j=0; j<clusArray[i].NumOfEl; j++)
            { checkpoint(); 
                jel = clusArray[i].NumsOfEl[j];
                
                
                if(!clusArray[i].IfMonoEl && state().listNumberNodesPerElement[jel] != inod0)
                {
                    inod0 = state().listNumberNodesPerElement[jel];
                    state().npFMGp = NC_ComputeGausseanPoints(state().csipFMgau, state().etapFMgau, state().weipFMgau, inod0, GaupFMBOrde);
                }
                
                
                NC_ComputeElementData(jel, 1, inodj, iveloj,
                                      ifadmij, jdofaddre, centerj, norvecj, zpotej,
                                      zveloj, crdelj);
                
                
                if(nrf_cl) NC_ReflectElementFMM(rfa_cl, mir_cl, rfdi_cl, inodj, 0,
                                                centerj, norvecj, zveloj, crdelj);
                if(nrf_cl && rfa_cl == -1) zpotej[0].nega();
                
                
                for(k=0; k<state().clulevarry[ilv].nPoinSpheLv; k++) zTele[k].set(0.0, 0.0);
                NC_ContributionTvecFMM(NCout, jel, state().clulevarry[ilv].nPoinSpheLv, center_i, crdelj,
                                       zpotej[0], zveloj, state().clulevarry[ilv].uvcsphe, zTele);
                
                for(k=0; k<state().clulevarry[ilv].nPoinSpheLv; k++) 
                    state().clulevarry[ilv].zwkT[ibg + k] += zTele[k];
            } 
            ibg += state().clulevarry[ilv].nPoinSpheLv;
        } 
    } 
}


void NC_ContributionTvecFMM
(
	ofstream& NCout,
	const int& nelj,		 
	int& npshe,				 
	Vector<double>& cencli,  
	Matrix<double>& crdelj,  
	Complex& zpote0,
	Vector<Complex>& zveloj, 
	double **uvshp,			 
	Vector<Complex>& zTele   
)
{
    int i, inodj, igp, jgp;
    double vljaco, wga, dw1;
    Vector<double> shfunx(NNODPE), shnds(NNODPE), shndt(NNODPE), dxds(NDIM),
    dxdt(NDIM), crdpoi(NDIM), elnorv(NDIM), vwk1(NDIM);
    Complex zbgao_0, zw1, zG, zH;
    
    
    inodj = state().listNumberNodesPerElement[nelj]; 
    
    
    for(igp=0; igp<state().npFMGp; igp++)
    { checkpoint(); 
        
        
        vljaco = NC_ComputeParameters(shfunx, shnds, shndt, dxds, dxdt,
                                      crdpoi, elnorv, zbgao_0, crdelj, 1,
                                      zveloj, inodj, state().csipFMgau[igp], state().etapFMgau[igp]);
        wga = state().weipFMgau[igp]*vljaco;
        
        
        for(jgp=0; jgp<npshe; jgp++)
        { checkpoint(); 
            vwk1 = cencli - crdpoi;
            dw1 = (Scprod_dim3_(vwk1, uvshp[jgp]))*state().waveNumbers_*state().harmonicTimeFactor_;
            zG.set(cos(dw1), sin(dw1));
            
            dw1 = (Scprod_dim3_(elnorv, uvshp[jgp]))*state().waveNumbers_;
            zH = zG*(dw1*wga);
            zG.mul_i(wga*state().harmonicTimeFactor_);

	    
	    if(state().ibval[nelj] == 1) {
	      zbgao_0 = state().zbvao0[nelj][0];
	      zbgao_0.div_i(state().rpfact_);
	      zTele[jgp] += zH*zbgao_0 - zG*zveloj[0];  
	    }
	    else
	      zTele[jgp] += zH*zpote0 - zG*zbgao_0;
        } 
    } 
    
    dw1 = state().waveNumbers_/PI4;
    for(i=0; i<npshe; i++) zTele[i].mul_r(dw1);
    if(!state().listElementProperty[nelj] && state().Tao_ != 1.0) for(i=0; i<npshe; i++) zTele[i].mul_r(state().Tao_);
}


void NC_WriteResultsObjectMesh 
(
	ofstream& NCout,
	Matrix<Complex>& cVpotele,
	Matrix<Complex>& cVeloele,
	Matrix<Complex>& cVeloelo,  
	Matrix<double>& rEninele
)
{
	int i, j, k, k1, iel, igr;
	int ibvi_0, ifadmii, inodi;
	double re1;
	Complex z0, z1, zpotsum;
	Vector<int> ndgrp(state().numElementGroups_), thingrp(state().numElementGroups_), mdgrp(state().numNodes_), nundgrp(state().numNodes_);
	Vector<int> idofaddre(2);
	Vector<double> centeri(NDIM), norveci(NDIM*(NNODPE + 1)), wkpoi(NDIM);
	Vector<Complex> admi(2);
	Vector<Complex> zbvi_0(NNODPE);
	Matrix<double> crdeli(NNODPE, NDIM);
	

	
	for(iel=0; iel<state().numElements_; iel++)
	{ checkpoint(); 
		if(state().listElementProperty[iel] == 2) continue;

		
		NC_ComputeElementData(iel, 0, inodi, ibvi_0,
            ifadmii, idofaddre, centeri, norveci, admi,
            zbvi_0, crdeli);

        switch(ibvi_0)
        {
        case 0: 
            cVpotele(iel, 0) = state().zrhs[idofaddre[0]];
            for(i=0; i<inodi; i++) cVeloelo(iel, i) = cVeloele(iel, i) = zbvi_0[i];
            if(ifadmii) 
            {
                z1 = (cVpotele(iel, 0)*admi[0])*state().Tao_;
                for(i=0; i<inodi; i++) cVeloele(iel, i) -= z1;
            }
            break;
        case 1: 
            cVpotele(iel, 0) = zbvi_0[0];
            for(i=0; i<inodi; i++) cVeloelo(iel, i) = cVeloele(iel, i) =
                state().zrhs[idofaddre[0]];
            break;
        case 2: 
            NC_Error_Exit_0(NCout, "transfer admittance boundary condition is not supported!");
        }

        for(i=0; i<inodi; i++) rEninele(iel, i) = 
            (cVpotele(iel, 0).re()*cVeloele(iel, i).im() - 
             cVpotele(iel, 0).im()*cVeloele(iel, i).re())*state().rpfact_*state().Tao_;
	} 

	
    sprintf(state().Dfname_3d, "be.out/be.%d/pBoundary",  state().currentFrequency_ + 1);
	state().Lu_pBoundary = local_open(state().Dfname_3d, "w");
    if(!state().Lu_pBoundary) NC_Error_Exit_1(NCout,  "Can not open the file ", state().Dfname_3d);
    sprintf(state().Dfname_3d, "be.out/be.%d/vBoundary",  state().currentFrequency_ + 1);
    state().Lu_vBoundary = local_open(state().Dfname_3d, "w");
    if(!state().Lu_vBoundary) NC_Error_Exit_1(NCout,  "Can not open the file ", state().Dfname_3d);
    
    








    
    fprintf(state().Lu_pBoundary, "%s\n", state().versionNumber_.c_str());
    fprintf(state().Lu_vBoundary, "%s\n", state().versionNumber_.c_str());
    

	
	for(igr=0; igr<state().numElementGroups_; igr++)
	{ checkpoint(); 
		thingrp[igr] = -1;
		for(j=0; j<state().numNodes_; j++) mdgrp[j] = -1;
		for(iel=0; iel<state().numElements_; iel++)
		{ checkpoint(); 
			if(state().listElementProperty[iel] == 2 || state().indexOfElementGroup[igr] != state().listElementsElementGroup[iel]) continue;
			if(thingrp[igr] == -1) thingrp[igr] = state().listElementProperty[iel];
			for(j=0; j<state().listNumberNodesPerElement[iel]; j++) mdgrp[state().elementsConnectivity[iel][j]] = 1;			
		}
		k1 = 0;
		for(j=0; j<state().numNodes_; j++) if(mdgrp[j] >= 0) k1++;
		ndgrp[igr] = k1;
	}

	
	int nelgrbou = 0;  
	for(igr=0; igr<state().numElementGroups_; igr++) if(ndgrp[igr] > 0) nelgrbou++;

    fprintf(state().Lu_pBoundary, "%5d\n", state().numElementGroups_);
    fprintf(state().Lu_vBoundary, "%5d\n", state().numElementGroups_);

	
	Matrix<Complex> zpregrp(state().numNodes_, 2); 
	Vector<Complex> zvelgrp(state().numNodes_);    
	Matrix<double> enegrp(state().numNodes_, 2);   

	for(igr=0; igr<state().numElementGroups_; igr++)
	{ checkpoint(); 
	  
		if(ndgrp[igr] == 0 || thingrp[igr] == -1) continue;
		for(j=0; j<state().numNodes_; j++)
		{ checkpoint(); 
			mdgrp[j] = 0;    
			nundgrp[j] = -1; 
			                 
			zvelgrp[j].set(0.0, 0.0);
			for(k=0; k<2; k++)
			{ checkpoint(); 
				zpregrp(j, k).set(0.0, 0.0);
				enegrp(j, k) = 0.0;
			}
		}
		for(iel=0; iel<state().numElements_; iel++)
		{ checkpoint(); 
		  
			if(state().listElementProperty[iel] == 2 || state().indexOfElementGroup[igr] != state().listElementsElementGroup[iel]) continue;
			
			for(j=0; j<state().listNumberNodesPerElement[iel]; j++)
			{ checkpoint(); 
				k = state().elementsConnectivity[iel][j];
				mdgrp[k]++;
				if(nundgrp[k] == -1) nundgrp[k] = state().extNumbersOfNodes[k];
				zpregrp(k, 0) += cVpotele(iel, 0);
				zvelgrp[k] += cVeloelo(iel, j);
				enegrp(k, 0) += rEninele(iel, j);
			}
		} 

		for(j=0; j<state().numNodes_; j++)
		{ checkpoint(); 
			if(mdgrp[j] > 0)
			{
				re1 = (double)mdgrp[j];
				zpregrp(j, 0).div_r(re1);
				zpregrp(j, 0).mul_i(state().rpfact_);
				zvelgrp[j].div_r(re1);
				enegrp(j, 0) /= re1;
			}
		}

        fprintf(state().Lu_pBoundary, "%5d %5d\n", state().indexOfElementGroup[igr], state().numberOfElementsInGroup[igr]);
        fprintf(state().Lu_vBoundary, "%5d %5d\n", state().indexOfElementGroup[igr], state().numberOfElementsInGroup[igr]);
        
        for(iel=0; iel<state().numElements_; iel++)
        { checkpoint(); 
            if(state().listElementProperty[iel] == 2 || state().indexOfElementGroup[igr] != state().listElementsElementGroup[iel]) continue;
	    
            Complex tmp;
            tmp=cVpotele(iel, 0);
            tmp.mul_i(state().rpfact_);
            fprintf(state().Lu_pBoundary, "%5d % E % E\n", state().extNumbersOfElements[iel], tmp.re(), tmp.im());
            tmp=cVeloele(iel, 0);
            fprintf(state().Lu_vBoundary, "%5d % E % E\n", state().extNumbersOfElements[iel], tmp.re(), tmp.im());
        }
	

		int j0 = -1;
		for(j=0; j<state().numNodes_; j++)
		{ checkpoint(); 
			if(nundgrp[j] == -1) continue;
			k1 = nundgrp[j];
			j0++;

			
			
			NC_Magnitude2dBdeg(state().Vmagdbph_pp_3d, zpregrp(j, 0), 0);
			NC_Magnitude2dBdeg(state().Vmagdbph_v_3d, zvelgrp[j], 1);
		} 
	} 
    












    local_close(state().Lu_pBoundary);
    local_close(state().Lu_vBoundary);

}


void NC_WriteResultsEvaluationGrid
(
	ofstream& NCout,
	Matrix<Complex>& cVpotele,  
	Matrix<Complex>& cVeloele   
)
{

  
	Complex zfacsourel, zquelinten, zprefree;
    FILE *lu_pEvalGrid, *lu_vEvalGrid;

    
    sprintf(state().Dfname_3d, "be.out/be.%d/pEvalGrid",  state().currentFrequency_ + 1);
    lu_pEvalGrid = local_open(state().Dfname_3d, "w");
    if(!lu_pEvalGrid) NC_Error_Exit_1(NCout,  "Can not open the file ", state().Dfname_3d);
    sprintf(state().Dfname_3d, "be.out/be.%d/vEvalGrid",  state().currentFrequency_ + 1);
    lu_vEvalGrid = local_open(state().Dfname_3d, "w");
    if(!lu_vEvalGrid) NC_Error_Exit_1(NCout,  "Can not open the file ", state().Dfname_3d);
    
    fprintf(lu_pEvalGrid, "%s\n", state().versionNumber_.c_str());
    fprintf(lu_vEvalGrid, "%s\n", state().versionNumber_.c_str());

	
	int inp, i, j, ndip, idip;
	double enresu, re1;
	Complex zprip, z1;
	Vector<double> crdip(NDIM), veip(NDIM);
	Vector<Complex> zprint(state().numNodesOfEvaluationMesh_);
	Vector<int> nuinnode(state().numNodesOfEvaluationMesh_);
	Matrix<Complex> zveint(state().numNodesOfEvaluationMesh_, NDIM + 1);
	Matrix<double> denint(state().numNodes_, NDIM);


	
	j = 0;
	for(i=0; i<state().numNodes_; i++) if(state().isNodeMeshOrEvalNode[i] == -1) nuinnode[j++] = i;

	
	for(i=0; i<state().numNodesOfEvaluationMesh_; i++)
	{ checkpoint(); 
		zprint[i].set(0.0, 0.0);
		for(j=0; j<NDIM; j++) zveint(i, j).set(0.0, 0.0);
	}

	

	
	if(state().numInternalPointsClusters_ > 0) 
	{
		
		NC_ContributionFMM(NCout, cVpotele, cVeloele, nuinnode, zprint,
			zveint);

		
		if(state().numIncidentPlaneWaves_ || state().numPointSources_) for(inp=0; inp<state().numNodesOfEvaluationMesh_; inp++)
		{ checkpoint(); 
			
			ndip = nuinnode[inp];
			for(i=0; i<NDIM; i++) crdip[i] = state().nodesCoordinates[ndip][i];

			z1 = NC_ContributionIncidentWaves(NCout, crdip, state().Zvelopoi_3d, 1);
			zprint[inp] += z1;
			for(j=0; j<NDIM; j++) zveint(inp, j) += state().Zvelopoi_3d[j];
		}
	}
	else 
	{
		for(inp=0; inp<state().numNodesOfEvaluationMesh_; inp++)
		{ checkpoint(); 
			
			ndip = nuinnode[inp];
			for(i=0; i<NDIM; i++) crdip[i] = state().nodesCoordinates[ndip][i];

			
			NC_ContributionTBEM(NCout, cVpotele, cVeloele, inp, ndip,
				crdip, zprint, zveint);

			
			if(state().numIncidentPlaneWaves_ || state().numPointSources_) 
			{
				z1 = NC_ContributionIncidentWaves(NCout, crdip, state().Zvelopoi_3d, 1);
				zprint[inp] += z1;
				for(j=0; j<NDIM; j++) zveint(inp, j) += state().Zvelopoi_3d[j];
			}
		} 
	} 

	
	
	
	int nugrinme = -1;
	for(i=0; i<state().numElements_; i++) if(state().listElementProperty[i] == 2)
	{
		nugrinme = state().listElementsElementGroup[i];
		break;
	}

	int ngrp = 0;
    if(state().numNodesOfEvaluationMesh_ > 0) ngrp = 1;
    fprintf(lu_vEvalGrid, "%5d\n", ngrp);

	if(state().numNodesOfEvaluationMesh_ > 0)
	{
        fprintf(lu_vEvalGrid, "%5d %5d\n", nugrinme, state().numNodesOfEvaluationMesh_);
	}

	
	for(inp=0; inp<state().numNodesOfEvaluationMesh_; inp++)
	  { checkpoint(); 
	    
	    ndip = state().extNumbersOfNodes[nuinnode[inp]];
	    
	    
	    for(j=0; j<NDIM; j++)
	      { checkpoint(); 
		NC_Magnitude2dBdeg(
				   state().Vmagdbph_v_3d, zveint(inp, j), 1
				   );
		veip[j] = state().Vmagdbph_v_3d[1];
	      }
	    
		
	    
	    
	    
	    fprintf(lu_vEvalGrid,"%5d %E %E %E %E %E %E\n", ndip,
		    zveint(inp,0).re(), zveint(inp,0).im(),
		    zveint(inp,1).re(), zveint(inp,1).im(),
		    zveint(inp,2).re(), zveint(inp,2).im());
	    
	} 
    
	fprintf(lu_pEvalGrid, "%5d\n", ngrp);
	if(state().numNodesOfEvaluationMesh_ > 0)
	  {
	    fprintf(lu_pEvalGrid, "%5d %5d\n", nugrinme, state().numNodesOfEvaluationMesh_);
	  }
	
	
	for(inp=0; inp<state().numNodesOfEvaluationMesh_; inp++)
	{ checkpoint(); 
		
		idip = nuinnode[inp];
		ndip = state().extNumbersOfNodes[idip];

		
		zprip = zprint[inp];
		zprip.mul_i(state().rpfact_);
        
		NC_Magnitude2dBdeg(state().Vmagdbph_pp_3d, zprip, 0);
		
		fprintf(lu_pEvalGrid, "%5d % E % E\n", ndip, zprip.re(), zprip.im());
	} 

	
	for(inp=0; inp<state().numNodesOfEvaluationMesh_; inp++)
	{ checkpoint(); 
		
		ndip = state().extNumbersOfNodes[nuinnode[inp]];

		
		z1 = zprint[inp];
		z1.mul_i(state().rpfact_);
		enresu = 0.0;
		for(j=0; j<NDIM; j++)
		{ checkpoint(); 
			re1 = z1.re()*zveint(inp, j).re() + z1.im()*zveint(inp, j).im();
			denint(nuinnode[inp], j) = re1;
			enresu += re1*re1;
		}
		enresu = sqrt(enresu);
	} 

	
    local_close(lu_pEvalGrid);
    local_close(lu_vEvalGrid);
}


Complex NC_ContributionIncidentWaves
(
	ofstream& NCout,
	Vector<double>&  sourpoi, 
	Vector<Complex>& zvelinw, 
	const int& ifcvelo        

)
{
    int i, j, kref;
    double re1, reffac, wavruim = state().harmonicTimeFactor_*state().waveNumbers_;
    Complex zrc(0.0, 0.0), zg, zini, zv;
    Vector<double> vect1(NDIM), vect2(NDIM);
    
    if(ifcvelo) for(i=0; i<NDIM; i++) zvelinw[i].set(0.0, 0.0);
    
    
    if(state().numIncidentPlaneWaves_ > 0)
    {
        Vector<double> sourposym(sourpoi, NDIM);
        if(state().numSymmetricPlanes_ > 0)
        {
            for(i=0; i<NDIM; i++) if(state().listSymmetryPlanes[i] != 0) sourposym[i] -=state().listSymmetryPlanesCoordinates[i];
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
                zrc += zg;
                
                if(ifcvelo)
                {
                    zv = zg;
                    zv.mul_i(wavruim);
                    for(j=0; j<NDIM; j++) zvelinw[j] += zv*vect1[j];
                }
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
                
                for(j=0; j<NDIM; j++) vect2[j] = sourpoi[j] - vect1[j];
                Vcnorm_dim3_(vect2, aglen);
                
                re1 = wavruim*aglen;
                re2 = reffac/PI4/aglen;
                zg.set(cos(re1)*re2, sin(re1)*re2);
                zg.mul_c(zini);
                zrc += zg;
                
                if(ifcvelo) 
                {
                    zv.set(-1.0/aglen, wavruim);
                    zv *= zg;
                    for(j=0; j<NDIM; j++) zvelinw[j] += zv*vect2[j];
                }
            }
        } 
    }
    
    return zrc;
}


void NC_ContributionTBEM
(
	ofstream& NCout,
	Matrix<Complex>& cVpotele,
	Matrix<Complex>& cVeloele,
	int& inp,                       
	int& ndip,                      
	Vector<double>& crdip,          
	Vector<Complex>& zprint,		
	Matrix<Complex>& zveint			
)
{
	
	int ie, i, j, inode, kref, nsbe, jsb, ireffac;
	double areli;
	Complex zprip;
	Vector<double> csip(NNODPE), etap(NNODPE);
	Vector<Complex> zveip(NDIM);

	
	int ivi_d, ifadmii;
	Complex zpotd, zpoto;
	Vector<int> idofaddre(2);
	Vector<double> centeri(NDIM), norveci(NDIM*(NNODPE + 1));
	Vector<Complex> zadmi(2);
	Vector<Complex> zvi_d(NNODPE);
	Matrix<double> crdeli(NNODPE, NDIM);

	
	Vector<double> xisbp(MSBE), etsbp(MSBE), fasbp(MSBE);
	Vector<int> ngsbp(MSBE);


	
	for(ie=0; ie<state().numElements_; ie++)
	{ checkpoint(); 
		if(state().listElementProperty[ie] == 2) continue;

		
		NC_ComputeElementData(ie, 0, inode, ivi_d,
			ifadmii, idofaddre, centeri, norveci, zadmi,
			zvi_d, crdeli);
		areli = state().areael[ie];

        zpoto = cVpotele(ie, 0);
        for(i=0; i<inode; i++) zvi_d[i] = cVeloele(ie, i);

		ivi_d = 0;
		for(i=0; i<inode; i++) 
		{ checkpoint(); 
			if(zvi_d[i].norm() > EPSY)
			{
				ivi_d = 1;
				break;
			}
		}

		
		ireffac = 1;
		for(kref=0; kref<state().numReflectionsOfElements_; kref++)
		{ checkpoint(); 
			
			if(kref > 0) NC_ReflectElement(kref, inode, 0, ireffac, centeri,
				norveci, zvi_d, crdeli);
			if(ireffac == -1) zpotd = zpoto*(-1.0); else zpotd = zpoto;
		
			
			if(inode == NETYP3)	for(i=0; i<inode; i++)
			{ checkpoint(); 
				csip[i] = CSI6[i];
				etap[i] = ETA6[i];
			}
			else for(i=0; i<inode; i++)
			{ checkpoint(); 
				csip[i] = CSI8[i];
				etap[i] = ETA8[i];
			}

			
			nsbe = NC_GenerateSubelements(NCout, ndip, crdip, ie, crdeli, areli,
				inode, csip, etap, inode, xisbp,
				etsbp, fasbp, ngsbp);

			
			zprip.set(0.0, 0.0);
			for(i=0; i<NDIM; i++) zveip[i].set(0.0, 0.0);

			
			for(jsb=0; jsb<nsbe; jsb++)
			{ checkpoint(); 
				zprip += NC_IntegrationTBEM(NCout, crdip, xisbp[jsb], etsbp[jsb], ngsbp[jsb],
					fasbp[jsb], inode, crdeli, zpotd, ivi_d,
					zvi_d, zveip, inode, ie);
			}

			
			if(state().Tao_ != 1.0)
			{
				zprip.mul_r(state().Tao_);
				for(j=0; j<NDIM; j++) zveip[j].mul_r(state().Tao_);
			}
			zprint[inp] += zprip;
			for(j=0; j<NDIM; j++) zveint(inp, j) += zveip[j]; 

		} 
	} 
}


void NC_ContributionFMM
(
	ofstream& NCout,
	Matrix<Complex>& cVpotele,
	Matrix<Complex>& cVeloele,
	Vector<int>& nuinnode,
	Vector<Complex>& zprint,		
	Matrix<Complex>& zveint			
)
{
	
	int ie, i, j, inode, nsbe, jsb, ipc, ic, isum, k, ig, ipp, inp, ndip;
	double areli, di0, dik, scprd, wavruim = state().waveNumbers_*state().harmonicTimeFactor_;
	Complex zprip, zmun, zi, zk, *ztvcwork = nullptr;

	switch(state().methodFMM_)
	{
	case 1: 
		ztvcwork = state().zT_vc;
		break;
	case 3: 

		ztvcwork = state().clulevarry[0].zwkT;

		
		state().ClustArray = state().clulevarry[0].ClastArLv;

		state().numOriginalClusters_ = state().clulevarry[0].nClustOLv;
		state().numOriginalReflectedClusters_ = state().clulevarry[0].nClustSLv;

		state().numExpansionTerms_ = state().clulevarry[0].nExpaTermLv;
		state().numIntegrationPointsUnitSphere_ = state().clulevarry[0].nPoinSpheLv;
		state().maxClusterRadiusBE_ = state().clulevarry[0].RadiMaxLv;
		state().avgClusterRadiusBE_ = state().clulevarry[0].RadiAveLv;
		state().minClusterRadiusBE_ = state().clulevarry[0].RadiMinLv;

		state().weisphe = state().clulevarry[0].weisphe;
		state().uvcsphe = state().clulevarry[0].uvcsphe;
		break;
	}

	Vector<double> csip(NNODPE), etap(NNODPE), dwk0(NDIM), Pm(state().numExpansionTerms_),
		crdip(NDIM);
	Vector<Complex> zveip(NDIM), zH0m(state().numExpansionTerms_);
	Vector<bool> iffarclus(state().numOriginalReflectedClusters_);

	
	int ivi_d, ifadmii;
	Complex zpotd;
	Vector<int> idofaddre(2);
	Vector<double> centeri(NDIM), norveci(NDIM*(NNODPE + 1));
	Vector<Complex> zadmi(2);
	Vector<Complex> zvi_d(NNODPE), zdt_vct(state().numIntegrationPointsUnitSphere_);
	Matrix<double> crdeli(NNODPE, NDIM);

	
	Vector<double> xisbp(MSBE), etsbp(MSBE), fasbp(MSBE);
	Vector<int> ngsbp(MSBE);

	
	int nrf_cl, rfa_cl;
	bool mir_cl;
	Vector<bool> rfdi_cl(NDIM);

    
	for(ipc=0; ipc<state().numInternalPointsClusters_; ipc++) { checkpoint(); 
	  
	  for(i=0; i<state().numOriginalReflectedClusters_; i++)
	    { checkpoint(); 
	      di0 = Dispoi_dim3_(state().ClustArray[i].CoorCent, state().ipcluarry[ipc].CoorCent);
	      dik = state().ClustArray[i].RadiClus + state().ipcluarry[ipc].RadiClus;
	      if(di0 > state().farFieldClusterFactor_*dik && di0 > state().minClusterDistance_) 
		iffarclus[i] = true; else iffarclus[i] = false;
	    }

	  
	  isum = 0;
	  for(ic=0; ic<state().numOriginalReflectedClusters_; ic++)
	    { checkpoint(); 
	      if(iffarclus[ic]) 
		{
		  
		  for(j=0; j<NDIM; j++) dwk0[j] = state().ipcluarry[ipc].CoorCent[j] - 
					  state().ClustArray[ic].CoorCent[j];
		  Vcnorm_dim3_(dwk0, di0);
		  dik = di0*state().waveNumbers_;
		  
		  
		  NC_SphericalHankel(NCout, zH0m, state().numExpansionTerms_, dik);
		  
		  
		  for(j=0; j<state().numIntegrationPointsUnitSphere_; j++)
		    { checkpoint(); 
		      scprd = Scprod_dim3_(dwk0, state().uvcsphe[j]);
		      
		      
		      NC_LegendrePolynomes(NCout, Pm, state().numExpansionTerms_, scprd);
		      
		      
		      zmun.set(0.0, 0.0);
		      zi.set(1.0, 0.0);
		      for(k=0; k<state().numExpansionTerms_; k++)
			{ checkpoint(); 
			  zmun += zi*zH0m[k]*Pm[k]*(double)(2*k + 1);
			  zi.mul_i(state().harmonicTimeFactor_); 
			}
		      zdt_vct[j] = zmun*ztvcwork[isum + j]*state().weisphe[j];
		    } 
		  
		      
		  for(ipp=0; ipp<state().ipcluarry[ipc].NumOfIps; ipp++)
		    { checkpoint(); 
		      
		      zprip.set(0.0, 0.0);
		      for(i=0; i<NDIM; i++) zveip[i].set(0.0, 0.0);
		      
		      
		      inp = state().ipcluarry[ipc].NumsOfIps[ipp];
		      ndip = nuinnode[inp];
		      
		      
		      for(i=0; i<NDIM; i++) crdip[i] = state().nodesCoordinates[ndip][i] - 
					      state().ipcluarry[ipc].CoorCent[i];
		      
		      
		      for(j=0; j<state().numIntegrationPointsUnitSphere_; j++) { checkpoint(); 
			scprd = Scprod_dim3_(crdip, state().uvcsphe[j])*wavruim;
			zi.set(cos(scprd), sin(scprd));
			zi *= zdt_vct[j];
			zprip += zi;
			for(k=0; k<NDIM; k++)
			  { checkpoint(); 
			    dik = wavruim*state().uvcsphe[j][k];
			    zk.set(-zi.im()*dik, zi.re()*dik);
			    zveip[k] += zk;
			  }
		      } 
		      
		      
		      zprint[inp] += zprip;
		      for(j=0; j<NDIM; j++) zveint(inp, j) += zveip[j];  
		    } 
		}
	      else 
		{
		  
		  nrf_cl = state().ClustArray[ic].nuref;
		  rfa_cl = state().ClustArray[ic].rffac;
		  if(nrf_cl)
		    {
		      mir_cl = state().ClustArray[ic].ifmirro;
		      for(i=0; i<NDIM; i++) rfdi_cl[i] = state().ClustArray[ic].ifrfdi[i];
		    }
		  
		  
		  for(ipp=0; ipp<state().ipcluarry[ipc].NumOfIps; ipp++)
		    { checkpoint(); 
		      
		      inp = state().ipcluarry[ipc].NumsOfIps[ipp];
		      ndip = nuinnode[inp];
		      
		      
		      for(i=0; i<NDIM; i++) crdip[i] = state().nodesCoordinates[ndip][i];
		      
		      
		      for(ig=0; ig<state().ClustArray[ic].NumOfEl; ig++)
			{ checkpoint(); 
			  ie = state().ClustArray[ic].NumsOfEl[ig]; 
			  
			  
			  NC_ComputeElementData(ie, 0, inode, ivi_d,
						ifadmii, idofaddre, centeri, norveci, zadmi,
						zvi_d, crdeli);
			  areli = state().areael[ie];
			  
			  zpotd = cVpotele(ie, 0);
			  for(k=0; k<inode; k++) zvi_d[k] = cVeloele(ie, k);
			  
			  ivi_d = 0;
			  for(k=0; k<inode; k++) 
			    { checkpoint(); 
			      if(zvi_d[k].norm() > EPSY)
				{
				  ivi_d = 1;
				  break;
				}
			    }
			  
			  
			  if(nrf_cl) NC_ReflectElementFMM(rfa_cl, mir_cl, rfdi_cl, inode, 0,
							  centeri, norveci, zvi_d, crdeli);
			  if(nrf_cl && rfa_cl == -1) zpotd.nega();
			  
			  
			  if(inode == NETYP3)	for(k=0; k<inode; k++)
						  { checkpoint(); 
						    csip[k] = CSI6[k];
						    etap[k] = ETA6[k];
						  }
			  else for(k=0; k<inode; k++)
				 { checkpoint(); 
				   csip[k] = CSI8[k];
				   etap[k] = ETA8[k];
				 }
			  
			  
			  nsbe = NC_GenerateSubelements(NCout, ndip, crdip, ie, crdeli, areli,
							inode, csip, etap, inode, xisbp,
							etsbp, fasbp, ngsbp);
			  
			  
			  zprip.set(0.0, 0.0);
			  for(i=0; i<NDIM; i++) zveip[i].set(0.0, 0.0);
			  
			  
			  for(jsb=0; jsb<nsbe; jsb++)
			    zprip += NC_IntegrationTBEM(NCout, crdip, xisbp[jsb], etsbp[jsb],
							ngsbp[jsb], fasbp[jsb], inode, crdeli,
							zpotd, ivi_d, zvi_d, zveip,
							inode, ie);
			  
			  
			  if(state().Tao_ != 1.0)
			    {
			      zprip.mul_r(state().Tao_);
			      for(j=0; j<NDIM; j++) zveip[j].mul_r(state().Tao_);
			    }
			  zprint[inp] += zprip;
			  for(j=0; j<NDIM; j++) zveint(inp, j) += zveip[j];
			      
			} 
		    } 
		} 
	      isum += state().numIntegrationPointsUnitSphere_;
	    } 
	} 
}


Complex NC_IntegrationTBEM
(
	ofstream& NCout,
	Vector<double>& crdip,		
	const double& xice,			
	const double& etce,			
	const int& ngse,			
	const double& fase,			
	const int& nvertj,			
	Matrix<double> crdele,		
	const Complex zpotee,		
	const int& ivi_d,			
	Vector<Complex>& zveloe,	
	Vector<Complex>& zveine,	
	const int& inode,			
	const int& numel			
)
{
    Complex zprine;
    int iforie, ngpoi, igp, j, i;
    double fase2, xis, ets, xio, eto, weih2, vjaco2, pleng, rk, re1, drdnx, vjacwe,
    wavruim = state().waveNumbers_*state().harmonicTimeFactor_;
    double csigau[36], etagau[36], weigau[36];
    Complex zvgp, zgrfu, z1, zgikr, zfac1, zfac2, zdgrdn, zdgrdx[NDIM],
    zdgdnx[NDIM];
    Vector<double> vect1p(NDIM);
    Vector<double> shfunx(NNODPE), shnds(NNODPE), shndt(NNODPE),
    elnorv(NDIM), crdpoi(NDIM), dxds(NDIM), dxdt(NDIM);
    
    
    fase2 = fase*fase;
    if(fabs(fase - 1.0) < EPSY) iforie = 1; 
    else iforie = 0; 
    
    
    ngpoi = NC_ComputeGausseanPoints(
                                     csigau, etagau, weigau, nvertj, ngse
                                     );
    
    
    zprine.set(0.0, 0.0);
    for(igp=0; igp<ngpoi; igp++)
    { checkpoint(); 
        
        xis = csigau[igp];
        ets = etagau[igp];
        if(iforie)
        {
            xio = xis;
            eto = ets;
            weih2 = weigau[igp];
        }
        else
        {
            xio = xice + xis*fase;
            eto = etce + ets*fase;
            weih2 = weigau[igp]*fase2;
        }
        
        
        vjaco2 = NC_ComputeParameters(shfunx, shnds, shndt, dxds, dxdt,
                                      crdpoi, elnorv, zvgp, crdele, ivi_d,
                                      zveloe, inode, xio, eto);
        
        
        for(j=0; j<NDIM; j++) vect1p[j] = crdpoi[j] - crdip[j];
        Vcnorm_dim3_(vect1p, pleng);
        rk = pleng*wavruim;
        re1 = pleng*PI4;
        zgrfu.set(cos(rk)/re1, sin(rk)/re1);
        
        z1.set(-1.0/pleng, wavruim);
        zgikr = zgrfu*z1;
        
        drdnx = Scprod_dim3_(vect1p, elnorv);
        z1.set(drdnx*(state().waveNumbers_*state().waveNumbers_ - 3.0/pleng/pleng),
               wavruim*3.0*drdnx/pleng);
        zfac1 = zgrfu*z1;
        
        zfac2 = zgikr/(-pleng);
        
        
        zdgrdn = zgikr*drdnx;
        for(j=0; j<NDIM; j++) zdgrdx[j] = zgikr*(-vect1p[j]);
        
        
        for(j=0; j<NDIM; j++) zdgdnx[j] = zfac1*vect1p[j] + zfac2*elnorv[j];
        
        
        vjacwe = vjaco2*weih2;
        
        
        z1 = zpotee*vjacwe;
        zprine += zdgrdn*z1;
        for(j=0; j<NDIM; j++) zveine[j] += zdgdnx[j]*z1;
        if(ivi_d) for(i=0; i<inode; i++)
        { checkpoint(); 
            z1 = zveloe[i]*(vjacwe*shfunx[i]);
            zprine -= zgrfu*z1;
            for(j=0; j<NDIM; j++) zveine[j] -= zdgrdx[j]*z1;
        }
        
    } 
    
    return zprine;
}


void NC_Magnitude2dBdeg
(
	Vector<double>& vmagdbph,  
	const Complex& zvalue,     
	const int& ifvelo          
)
{
    vmagdbph[0] = zvalue.norm();
    vmagdbph[2] = atan2(zvalue.im(), zvalue.re())*180/PI;
    
    switch(ifvelo)
    {
        case 0:  
            if(vmagdbph[0] >= PREREF)
            {
                vmagdbph[1] = 20.0*(log10(vmagdbph[0]) - log10(PREREF));
            }
            else
            {
                vmagdbph[1] = 0.0;
            }
            break;
        case 1:
        default: 
            if(vmagdbph[0] >= VELREF)
            {
                vmagdbph[1] = 20.0*(log10(vmagdbph[0]) - log10(VELREF));
            }
            else
            {
                vmagdbph[1] = 0.0;
            }
            break;
    }
}
