














void NC_ReflectElementFMM
(
    int& rfa_cl,             
	bool& mir_cl,            
	Vector<bool>& rfdi_cl,   
	const int& inodj,        
	const int& ibvj_0,       
			                 
							 
	Vector<double>& centerj, 
	Vector<double>& norvecj, 
	Vector<Complex>& zbvj_0, 
	Matrix<double>& crdelj   
)
{
	int i, nrdir, nod1, nod2, ntime;
	double xi;
    Complex zbl;

	
	if(ibvj_0 <= 1 && rfa_cl == -1) for(i=0; i<inodj; i++) zbvj_0[i].nega();

	
	for(nrdir=0; nrdir<NDIM; nrdir++)
	{ checkpoint(); 
		if(!rfdi_cl[nrdir]) continue;
		centerj[nrdir] = 2.0*state().listSymmetryPlanesCoordinates[nrdir] - centerj[nrdir];
		norvecj[nrdir] = -norvecj[nrdir];
		for(i=0; i<inodj; i++)
		{ checkpoint(); 
			norvecj[NDIM*(i + 1) + nrdir] = -norvecj[NDIM*(i + 1) + nrdir];
			crdelj(i, nrdir) = 2.0*state().listSymmetryPlanesCoordinates[nrdir] - crdelj(i, nrdir);
		}
	}

	
	if(mir_cl)
	{
		nod1 = 0;
		nod2 = 1;
		ntime = 0;
IterBegin_rfFMB:
		ntime++;
		for(i=0; i<NDIM; i++) 
		{ checkpoint(); 
			xi = crdelj(nod1, i);
			crdelj(nod1, i) = crdelj(nod2, i);
			crdelj(nod2, i) = xi;
			xi = norvecj[NDIM*(nod1 + 1) + i];
			norvecj[NDIM*(nod1 + 1) + i] = norvecj[NDIM*(nod2 + 1) + i];
			norvecj[NDIM*(nod2 + 1) + i] = xi;
		}
		zbl = zbvj_0[nod1];
		zbvj_0[nod1] = zbvj_0[nod2];
		zbvj_0[nod2] = zbl;
		if(inodj == NETYP4 && ntime == 1) 
		{
			nod1 = 2;
			nod2 = 3;
			goto IterBegin_rfFMB;
		}
	}
}


void NC_ComputeGlobalCoordinates
(
    Vector<double>& crdpi, Vector<double>& shfunx,
    Matrix<double>& crdel, const double& s, const double& t,
    const int& inode)
{
	int i, j;
	double s1, s2, t1, t2;

	switch(inode) {
	case 3:
		shfunx[0] = s;
		shfunx[1] = t;
		shfunx[2] = 1.0 - s - t;
		break;
	case 4:
		s1 = 0.25*(s + 1.0);
		s2 = 0.25*(s - 1.0);
		t1 = t + 1.0;
		t2 = t - 1.0;
		shfunx[0] = s1*t1;
		shfunx[1] = -s2*t1;
		shfunx[2] = s2*t2;
		shfunx[3] = -s1*t2;
		break;
	default:
		NC_Error_Exit_0(runtime->jobLog, "number of nodes of an element must be 3 or 4!");
	}

	for(i=0; i<NDIM; i++)
	{ checkpoint(); 
		crdpi[i] = 0.0;
		for(j=0; j<inode; j++) crdpi[i] += shfunx[j]*crdel(j, i);
	}
}


double NC_ComputeUnitVectorOnElement
(
    Vector<double>& elnorv, Vector<double>& shnds,
    Vector<double>& shndt, Vector<double>& dxds,
    Vector<double>& dxdt, Matrix<double>& crdel,
    const double& s, const double& t, const int& inode)
{
	int i, j;
	double length;

	switch(inode) {
	case 3:
		shnds[0] = shndt[1] = 1.0;
		shnds[1] = shndt[0] = 0.0;
		shnds[2] = shndt[2] = -1.0;
		break;
	case 4:
		shnds[0] = 0.25*(t + 1.0);
		shnds[2] = 0.25*(t - 1.0);
		shnds[1] = -shnds[0];
		shnds[3] = -shnds[2];

		shndt[0] = 0.25*(s + 1.0);
		shndt[2] = 0.25*(s - 1.0);
		shndt[1] = -shndt[2];
		shndt[3] = -shndt[0];
		break;
	default:
		NC_Error_Exit_0(runtime->jobLog, "number of nodes of an element must be 3 or 4!");
	}

	
	for(i=0; i<NDIM; i++) 
	{ checkpoint(); 
		dxds[i] = dxdt[i] = 0.0;
		for(j=0; j<inode; j++)
		{ checkpoint(); 
			dxds[i] += shnds[j]*crdel(j, i);
			dxdt[i] += shndt[j]*crdel(j, i);
		}
	}

	elnorv.mul_3(dxds, dxdt);
	Vcnorm_dim3_(elnorv, length);

	return length;
}


double NC_ReflectPointVector
(
	Vector<double>& po_ve, 
	const int& kref,       
	double& reffac,        
	const int &ifpoi       
				           
)
{
	int i, nrdir=0, i0=0;
	int nrefdi[] = {0, 0, 1, 0, 2, 0, 1, 0};

	if(kref == 0)
	{
		return(reffac);
	}
	else if(kref < 0 || kref > 7)
		NC_Error_Exit_0("kref must be equal to 0, 1, ... 7!");

	
	switch(state().numReflectionsOfElements_)
	{
	case 8:  
		nrdir = nrefdi[kref];
		break;

	case 4:  
		for(i=0; i<NDIM; i++) if(state().listSymmetryPlanes[i] == 0)
		{
			i0 = i + 1;
			break;
		}
		nrdir = i0 + nrefdi[kref];
		if(nrdir >= NDIM) nrdir -= NDIM;
		break;
	case 2:  
		for(i=0; i<NDIM; i++) if(state().listSymmetryPlanes[i] != 0)
		{
			nrdir = i;
			break;
		}
		break;
	case 1:  
		return(reffac);
		break;
	default:
			NC_Error_Exit_0(runtime->jobLog, "state().numReflectionsOfElements_ must be equal to 8, 4, 2 or 1!");
	}

	
	if(ifpoi) 
	{
		po_ve[nrdir] = 2.0*state().listSymmetryPlanesCoordinates[nrdir] - po_ve[nrdir];
	}
	else      
	{
		po_ve[nrdir] = - po_ve[nrdir];
	}
	if(state().listSymmetryPlanes[nrdir] < 0) reffac = - reffac;

	return reffac;
}


void NC_ComputeElementData
(
	const int& jel,          
	const int& icase,        
			                 
							 
							 
							 
							 
							 
							 
	int& inodj,              
	int& ibvj_0,             
			                 
	int& ifadmij,            
	Vector<int>& jdofaddre,  
	Vector<double>& centerj, 
	Vector<double>& norvecj, 
	Vector<Complex>& admj,   
	Vector<Complex>& zbvj_0, 
			                 
	Matrix<double>& crdelj   
)
{
	int i, j;

	inodj = state().listNumberNodesPerElement[jel];

	
	switch(state().ibval[jel]) 
	{
	case 0:         
		ibvj_0 = 0;  
		ifadmij = 0;
		break;
	case 1:         
		ibvj_0 = 1;
		ifadmij = 0;
		break;
	case 2:         
		ibvj_0 = 0;  
		ifadmij = 1;
		break;
	case 3:         
		ibvj_0 = 2;
		ifadmij = 0;
		break;
	case 4:         
		ibvj_0 = 2;
		ifadmij = 1;
		break;
	}

	
	jdofaddre[0] = state().jelist[jel][0];
	jdofaddre[1] = state().jelist[jel][1];

	
	for(i=0; i<NDIM; i++) centerj[i] = state().centel[jel][i];
	for(j=0; j<inodj + 1; j++) for(i=0; i<NDIM; i++) 
		norvecj[j*NDIM + i] = state().elenor[jel][j*NDIM + i];

	
	for(i=0; i<inodj; i++) zbvj_0[i] = state().zbval0[jel][i];

	
	for(i=0; i<inodj; i++) for(j=0; j<NDIM; j++) 
		crdelj(i, j) = state().nodesCoordinates[state().elementsConnectivity[jel][i]][j];

    if(ifadmij)
    {
        admj[0] = state().zbval1[jel];
    }
    else
    {
        admj[0] = 0.0;
    }

	if(icase == 1) 
	{
		Complex ztmp;
        if(ibvj_0 == 1) 
        {
            for(i=0; i<inodj; i++) zbvj_0[i] = state().zrhs[jdofaddre[0]];
        }
        else 
        {
            if(ifadmij)
            {
                ztmp = (admj[0]*state().zrhs[jdofaddre[0]])*state().Tao_;
                for(i=0; i<inodj; i++) zbvj_0[i] -= ztmp;
                ibvj_0 = 1;
            }
            else
            {
                ibvj_0 = 0;
                for(i=0; i<inodj; i++)
                { checkpoint(); 
                    if(zbvj_0[i].norm() > EPSY)
                    {
                        ibvj_0 = 1;
                        break;
                    }
                }
            }
            admj[0] = state().zrhs[jdofaddre[0]];
        }

	} 
}


void NC_ReflectElement
(
	const int& kref,         
	const int& inodj,        
	const int& ibvj_0,       
			                 
							 
	int& ireffac,            
	Vector<double>& centerj, 
	Vector<double>& norvecj, 
	Vector<Complex>& zbvj_0, 
	Matrix<double>& crdelj   
)
{
	int i, nrdir=0, i0=0, nod1, nod2, ntime;
	double xi;
    Complex zbl;
	int nrefdi[] = {0, 0, 1, 0, 2, 0, 1, 0};

	if(kref == 0)
	{
		return;
	}
	else if(kref < 0 || kref > 7)
			NC_Error_Exit_0("kref must be equal to 0, 1, ... 7!");

	
	switch(state().numReflectionsOfElements_)
	{
	case 8:  
		nrdir = nrefdi[kref];
		break;
	case 4:  
		for(i=0; i<NDIM; i++) if(state().listSymmetryPlanes[i] == 0) 
		{
			i0 = i + 1;
			break;
		}
		nrdir = i0 + nrefdi[kref];
		if(nrdir >= NDIM) nrdir -= NDIM;
		break;
	case 2:  
		for(i=0; i<NDIM; i++) if(state().listSymmetryPlanes[i] != 0)
		{
			nrdir = i;
			break;
		}
		break;
	case 1:  
		return;
		break;
	default:
		NC_Error_Exit_0(runtime->jobLog, "state().numReflectionsOfElements_ must be equal to 8, 4, 2 or 1!");
	}

	
	if(state().listSymmetryPlanes[nrdir] < 0)
	{
		ireffac = -ireffac;
		if(ibvj_0 == 0) for(i=0; i<inodj; i++) zbvj_0[i].nega();
	}

	
	centerj[nrdir] = 2.0*state().listSymmetryPlanesCoordinates[nrdir] - centerj[nrdir];
	norvecj[nrdir] = -norvecj[nrdir];
	for(i=0; i<inodj; i++)
	{ checkpoint(); 
		norvecj[NDIM*(i + 1) + nrdir] = -norvecj[NDIM*(i + 1) + nrdir];
		crdelj(i, nrdir) = 2.0*state().listSymmetryPlanesCoordinates[nrdir] - crdelj(i, nrdir);
	}

	
	nod1 = 0;
	nod2 = 1;
	ntime = 0;
IterBegin_rfe:
	ntime++;
	for(i=0; i<NDIM; i++) 
	{ checkpoint(); 
		xi = crdelj(nod1, i);
		crdelj(nod1, i) = crdelj(nod2, i);
		crdelj(nod2, i) = xi;
		xi = norvecj[NDIM*(nod1 + 1) + i];
		norvecj[NDIM*(nod1 + 1) + i] = norvecj[NDIM*(nod2 + 1) + i];
		norvecj[NDIM*(nod2 + 1) + i] = xi;
	}
	zbl = zbvj_0[nod1];
	zbvj_0[nod1] = zbvj_0[nod2];
	zbvj_0[nod2] = zbl;
	if(inodj == NETYP4 && ntime == 1) 
	{
		nod1 = 2;
		nod2 = 3;
		goto IterBegin_rfe;
	}
}


int NC_ComputeGausseanPoints
(
    double* csigau,    
    double* etagau,    
    double* weigau,    
    const int& inodj,  
    const int& norder  
)
{
	int ngp=0, i, j;
	Vector<double> coordgau(norder), weitgau(norder);

	switch(inodj) 
	{
	case 3:
		switch(norder)
		{
		case 1:
			ngp = 1;
			csigau[0] = GAUCORWEI_TR1[0][0];
			etagau[0] = GAUCORWEI_TR1[0][1];
			weigau[0] = GAUCORWEI_TR1[0][2];
			break;
		case 2:
			ngp = 4;
			for(i=0; i<ngp; i++) 
			{ checkpoint(); 
				csigau[i] = GAUCORWEI_TR4[i][0];
				etagau[i] = GAUCORWEI_TR4[i][1];
				weigau[i] = GAUCORWEI_TR4[i][2];
			}
			break;
		case 3:
			ngp = 7;
			for(i=0; i<ngp; i++) 
			{ checkpoint(); 
				csigau[i] = GAUCORWEI_TR7[i][0];
				etagau[i] = GAUCORWEI_TR7[i][1];
				weigau[i] = GAUCORWEI_TR7[i][2];
			}
			break;
		case 4:
		case 5:
		case 6:
		case 7:
			ngp = 13;
			for(i=0; i<ngp; i++) 
			{ checkpoint(); 
				csigau[i] = GAUCORWEI_TR13[i][0];
				etagau[i] = GAUCORWEI_TR13[i][1];
				weigau[i] = GAUCORWEI_TR13[i][2];
			}
			break;
		default:
			NC_Error_Exit_0(runtime->jobLog, "norder must be between 1 and 7!");
		} 
		break;
	case 4:

		BLGauCooWe(runtime->jobLog, norder, coordgau, weitgau);

		ngp = 0;
		for(i=0; i<norder; i++) for(j=0; j<norder; j++)
		{ checkpoint(); 
			csigau[ngp] = coordgau[i];
			etagau[ngp] = coordgau[j];
			weigau[ngp] = weitgau[i]*weitgau[j];
			ngp++;
		}
		break;
	default:
		NC_Error_Exit_0(runtime->jobLog, "inodj must be equal to 3 or 4!");
	} 

	return ngp;
}


double NC_ComputeParameters
(
    Vector<double>& shapfn,  
    Vector<double>& shapds,  
    Vector<double>& shapdt,  
    Vector<double>& dxdxi,   
    Vector<double>& dxdet,   
    Vector<double>& crdgpi,  
    Vector<double>& elnorm,  
    Complex& zbvgau_0,       
    Matrix<double>& crden,   
    const int& ifcrhs,       
    Vector<Complex>& zbvj_0, 
    const int& inode,        
    const double& ss,        
    const double& tt         
)
{
	int i;
	double vjaco;

	
	NC_ComputeGlobalCoordinates(crdgpi, shapfn, crden, ss, tt, inode);

	
    vjaco = NC_ComputeUnitVectorOnElement(elnorm, shapds, shapdt, dxdxi, dxdet, crden,
				ss, tt, inode);

	
	zbvgau_0.set(0.0, 0.0);
	if(ifcrhs)
	{
		for(i=0; i<inode; i++)
		{ checkpoint(); 
			zbvgau_0 += zbvj_0[i]*shapfn[i]; 
		}
	}

	return vjaco;
}


int NC_GenerateSubelements
(
	ofstream& NCout,
	const int& ndip,        
	Vector<double>& crdip,  
	const int& je,          
	Matrix<double>& crdelj, 
	const double& arelj,    
	const int& nvertj,      
	Vector<double>& csij,   
	Vector<double>& etaj,   
	const int& jnode,       
	Vector<double>& xisbp,  
			                
	Vector<double>& etsbp,  
			                
	Vector<double>& fasbp,  
	Vector<int>& ngsbp      
)
{
	const int MNSE = 60, MVX = 4, NSE = 4; 

	int nsbe = -1, nsfl = 1, i, j, ndie, nsel, idi, nsu, j1, j2, jr = 0, nsf0,
		ngaumax = 6, ngaumin = 2;
	double tolf = 1.3,
    faclin = 2.0, arels, scent, tcent, ratdis, xi, et, dxi = 0.0, det,
		disfac, err_g, err_h, err_e, gauaccu = 0.001;
	Vector<double> crdpoip(NDIM), xisp(MVX*2), etsp(MVX*2);
	Matrix<double> xisfp(MNSE, MVX), etsfp(MNSE, MVX), xisep(MNSE, MVX),
		etsep(MNSE, MVX);
	Vector<double> shfunx(NNODPE);

	for(j=0; j<nvertj; j++)
	{ checkpoint(); 
		xisfp(0, j) = csij[j];
		etsfp(0, j) = etaj[j];
	}

	
Begin_Subel:
	ndie = 0;      
	faclin *= 0.5; 
	arels = arelj*faclin*faclin; 
	nsel = nsfl;   

	
	for(idi=0; idi<nsel; idi++) for(j=0; j<nvertj; j++)
	{ checkpoint(); 
		xisep(idi, j) = xisfp(idi, j);
		etsep(idi, j) = etsfp(idi, j);
	}

	
	idi = 0; 
Begin_Idi:

	
	scent = tcent = 0.0;
	for(j=0; j<nvertj; j++)
	{ checkpoint(); 
		scent += xisep(idi, j); 
		tcent += etsep(idi, j);
	}
	scent /= (double)nvertj;
	tcent /= (double)nvertj;
	NC_ComputeGlobalCoordinates(crdpoip, shfunx, crdelj, scent, tcent, jnode); 

	
	ratdis = BLdispoi(crdpoip, crdip)/sqrt(arels); 

	if(ratdis < tolf) 
	{
		ndie++;
		if(ndie > 15)
		{
			runtime->quiet << endl;
			if(ndip >= 0)
				runtime->quiet << "number of the source node    = " << state().extNumbersOfNodes[ndip] << endl;
			runtime->quiet << "coordinates of the source point: " << 
				crdip[0] << " " << crdip[1] << " " << crdip[2] << endl;
			NCout << endl;
			if(ndip >= 0)
				NCout << "number of the source node    = " << state().extNumbersOfNodes[ndip] << endl;
			NCout << "coordinates of the source point: " << 
				crdip[0] << " " << crdip[1] << " " << crdip[2] << endl;
			NC_Error_Exit_1(NCout, 
				"number of subels. which are subdivided in a loop must <= 15!",
				"number of the element = ", state().extNumbersOfElements[je]);
		}
		nsfl = ndie*NSE;
		nsf0 = nsfl - NSE;
		
		
		for(j=0; j<nvertj; j++)
		{ checkpoint(); 
			j1 = j + 1;
			if(j1 == nvertj) j1 = 0;
			xisp[j] = xisep(idi, j);
			xisp[j + nvertj] = (xisep(idi, j) + xisep(idi, j1))/2.0;
			etsp[j] = etsep(idi, j);
			etsp[j + nvertj] = (etsep(idi, j) + etsep(idi, j1))/2.0;
		}

		
		for(j=0; j<nvertj; j++)
		{ checkpoint(); 
			nsu = nsf0 + j;
			j1 = j + nvertj;
			j2 = j1 - 1;
			if(j2 < nvertj) j2 += nvertj;
			xisfp(nsu, 0) = xisp[j];
			xisfp(nsu, 1) = xisp[j1];
			etsfp(nsu, 0) = etsp[j];
			etsfp(nsu, 1) = etsp[j1];
			if(nvertj == NETYP4) 
			{
				xisfp(nsu, 2) = scent;
				xisfp(nsu, 3) = xisp[j2];
				etsfp(nsu, 2) = tcent;
				etsfp(nsu, 3) = etsp[j2];
			}
			else 
			{
				xisfp(nsu, 2) = xisp[j2];
				etsfp(nsu, 2) = etsp[j2];
				xisfp(nsf0 + NSE - 1, j) = xisp[j1];
				etsfp(nsf0 + NSE - 1, j) = etsp[j1];
			}
		} 
	}
	else 
	{
		nsbe++;
		if(nsbe >= MSBE)
		{
			if(ndip >= 0)
			{
				runtime->quiet << "number of the source node    = " << state().extNumbersOfNodes[ndip] << endl;
				NCout << "number of the source node    = " << state().extNumbersOfNodes[ndip] << endl;
			}
			runtime->quiet << "coordinates of the source point: " << 
				crdip[0] << " " << crdip[1] << " " << crdip[2] << endl;
			NCout << "coordinates of the source point: " << 
				crdip[0] << " " << crdip[1] << " " << crdip[2] << endl;
			NC_Error_Exit_1(NCout, "nsbe must be < MSBE(= 110)", 
				"number of the element = ", state().extNumbersOfElements[je]);
		}

		if(nvertj == NETYP4) 
		{
			
			xisbp[nsbe] = (xisep(idi, 0) + xisep(idi, 1) + xisep(idi, 2) + 
				xisep(idi, 3))/4.0;
			etsbp[nsbe] = (etsep(idi, 0) + etsep(idi, 1) + etsep(idi, 2) + 
				etsep(idi, 3))/4.0;

			
			fasbp[nsbe] = faclin;
		}
		else 
		{
			
			xi = (xisep(idi, 0) + xisep(idi, 1) + xisep(idi, 2))/3.0;
			et = (etsep(idi, 0) + etsep(idi, 1) + etsep(idi, 2))/3.0; 

			
			for(j=0; j<nvertj; j++)
			{ checkpoint(); 
				dxi = xi - xisep(idi, j);
				det = et - etsep(idi, j);
				if(fabs(dxi - det) < 1.0e-8)
				{
					jr = j;
					break;
				}
			}

			
			xisbp[nsbe] = xisep(idi, jr);
			etsbp[nsbe] = etsep(idi, jr);

			
			if(dxi >= 0.0) fasbp[nsbe] = faclin; else fasbp[nsbe] = -faclin; 
		}

		
		disfac = 0.5/ratdis;
		ngsbp[nsbe] = ngaumax;
		for(i=ngaumin; i<=ngaumax; i++)
		{ checkpoint(); 
			Err_G_(i, disfac, j, err_g);
			Err_H_(i, disfac, j, err_h);
			Err_E_(i, disfac, j, err_e);
			if(err_e < gauaccu && err_h < gauaccu && err_g < gauaccu)
			{
				ngsbp[nsbe] = i;
				break;
			}
		}
	} 

	idi++;
	if(idi < nsel) goto Begin_Idi;

	if(ndie > 0) goto Begin_Subel;


	return (nsbe + 1);
}


void NC_ComputeUnitVectorOnUnitSphere
(
	ofstream& NCout,
	const int& np_thet,  
	const int& np_phi,   
	double* arg_weisphe,	 
	double** arg_uvcsphe     
)
{
	int i, j, k;
	double phi, cothet, sithet, delphi;
	Vector<double> crdgpth(np_thet), wegpth(np_thet);

	
	BLGauCooWe(NCout, np_thet, crdgpth, wegpth);
	delphi = 2.0*PI/(double)(np_phi); 

	
	k = 0;
	for(i=0; i<np_thet; i++)
	{ checkpoint(); 
		cothet = crdgpth[i];
		sithet = sqrt(1.0 - cothet*cothet);
		for(j=0; j<np_phi; j++)
		{ checkpoint(); 
			arg_weisphe[k] = wegpth[i]*delphi/PI4;
			phi = delphi*(double)(j);
			arg_uvcsphe[k][0] = sithet*cos(phi);
			arg_uvcsphe[k][1] = sithet*sin(phi);
			arg_uvcsphe[k++][2] = cothet;
		}
	}
}


void NC_SphericalHankel
(
	ofstream& NCout,
	Vector<Complex>& zH_n,	
	const int& norder,      
	const double& argu      
)
{
    if(norder < 2) NC_Error_Exit_1(NCout,
                              "Highst order the spherical Hankel funktions must greater than 1",
                              "Highst order = ", norder);
    
    int i, j=1;
    double dnu, gnu, errgn=1.0, di, Dj, Cj, aj, bj;
    double epsign = 1.0e-9, dp;
    Vector<double> dgn(norder), ggn(norder), dY_n(norder);
    
    
    dY_n[0] = -cos(argu)/argu;
    dY_n[1] = -(cos(argu)/argu + sin(argu))/argu;
    
    for(i=2; i<norder; i++) dY_n[i] = dY_n[i - 1]*(double)(2*i - 1)/argu -
        dY_n[i - 2];
    
    
    dnu = (double)(norder - 1);
    
    di=(double)(2*(dnu+1)+1)/argu;
    Cj=di;
    Dj=0.0;
    while(errgn > epsign)
    { checkpoint(); 
        aj=-1.0;
        bj=(double)(2*(dnu+j+1)+1)/argu;
        Dj=bj+aj*Dj;
        if(Dj==0){Dj=1e-30;}
        Dj=1/Dj;
        Cj=bj+aj/Cj;
        if(Cj==0){Cj=1e-30;}
        di=di*Cj*Dj;
        errgn = fabs(Cj*Dj-1.0);
        j=j+1;
    }
    
    gnu=dnu/argu-1/di;
    
    ggn[norder - 1] = 1.0;
    dgn[norder - 1] = gnu;
    for(i=norder-2; i>=0; i--)
    { checkpoint(); 
        di = (double)(i);
        ggn[i] = (di + 2.0)/argu*ggn[i + 1] + dgn[i + 1];
        dgn[i] = di/argu*ggn[i] - ggn[i + 1];
    }
    
    if(fabs(ggn[0]) > 1.0e-5)
    {
        dp = sin(argu)/argu/ggn[0];
    }
    else
    {
        dp = (cos(argu) - sin(argu)/argu)/argu/dgn[0];
    }
    
    for(i=0; i<norder; i++) zH_n[i].set(dp*ggn[i], state().harmonicTimeFactor_*dY_n[i]);
}


void NC_LegendrePolynomes
(
	ofstream& NCout,
	Vector<double>& PLe,	
	const int& norder,      
	const double& argu      
)
{
	if(norder < 2) NC_Error_Exit_1(NCout, 
		"Highst order of the Legendre polynomes must greater than 1",
		"Highst order = ", norder);

	PLe[0] = 1.0;
	PLe[1] = argu;

	for(int i=2; i<norder; i++) PLe[i] = (PLe[i - 1]*argu*(double)(2*i - 1) -
		PLe[i - 2]*(double)(i - 1))/(double)(i);
}