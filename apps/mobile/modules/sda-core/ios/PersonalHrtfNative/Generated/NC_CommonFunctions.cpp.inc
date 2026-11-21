














void NC_MatrixVectorMultiplication(Vector<Complex>&, Vector<Complex>&);
void NC_ComputeScalingVector(ofstream&);
void NC_IncompleteLUDecomposition(ofstream&);
void NC_DeleteIncompleteLUMatrices();
void NC_IncompleteLUForBack(ofstream&, Vector<Complex>&, Vector<Complex>&);
void NC_IncompleteLUForBack();












void NC_Error_Exit_0 
(
	ofstream& NCout,
	const string& errorMessage
)
{
	runtime->quiet << errorMessage << endl;
	NCout << errorMessage << endl;
	fail_exit(1);
}

void NC_Error_Warning_0 
(
	ofstream& NCout,
	const string& errorMessage
)
{
	runtime->quiet << errorMessage << "\n" << endl;
	NCout << errorMessage << "\n" << endl;
}

void NC_Error_Exit_0 
(
	const string& errorMessage
)
{
	runtime->quiet << errorMessage << endl;
	fail_exit(1);
}

void NC_Error_Warning_0 
(
	const string& errorMessage
)
{
	runtime->quiet << errorMessage << "\n" << endl;
}


void NC_Error_Exit_1 
(
	ofstream& NCout,
	const string& errorMessage,
	const string& str1,
	const int& int1
)
{
	runtime->quiet << errorMessage << endl;
	runtime->quiet << str1 << int1 << endl;
	NCout << errorMessage << endl;
	NCout << str1 << int1 << endl;
	fail_exit(1);
}

void NC_Error_Exit_1 
(
	ofstream& NCout,
	const string& errorMessage,
	const string& str1,
	const double& d1
)
{
	runtime->quiet << errorMessage << endl;
	runtime->quiet << str1 << d1 << endl;
	NCout << errorMessage << endl;
	NCout << str1 << d1 << endl;
	fail_exit(1);
}

void NC_Error_Exit_1 
(
    ofstream& NCout,
    const string& errorMessage,
    const string& str1
)
{
    runtime->quiet << errorMessage << " " << str1 << endl;
    NCout << errorMessage << " " << str1 << endl;
	fail_exit(1);
}

void NC_Error_Warning_1 
(
	ofstream& NCout,
	const string& errorMessage,
	const string& str1,
	const int& int1
)
{
	runtime->quiet << "\nWarning: " << errorMessage << endl;
	runtime->quiet << str1 << int1 << "\n" << endl;
	NCout << "\nWarning: " << errorMessage << endl;
	NCout << str1 << int1 << "\n" << endl;
}

void NC_Error_Warning_1 
(
	ofstream& NCout,
	const string& errorMessage,
	const string& str1,
	const double& d1
)
{
	runtime->quiet << "\nWarning: " << errorMessage << endl;
	runtime->quiet << str1 << d1 << "\n" << endl;
	NCout << "\nWarning: " << errorMessage << endl;
	NCout << str1 << d1 << "\n" << endl;
}

void NC_Error_Exit_1 
(
	const string& errorMessage,
	const string& str1,
	const int& int1
)
{
	runtime->quiet << errorMessage << endl;
	runtime->quiet << str1 << int1 << endl;
	fail_exit(1);
}

void NC_Error_Exit_1 
(
	const string& errorMessage,
	const string& str1,
	const double& d1
)
{
	runtime->quiet << errorMessage << endl;
	runtime->quiet << str1 << d1 << endl;
	fail_exit(1);
}

void NC_Error_Exit_1 
(
    const string& errorMessage,
    const string& str1
)
{
	runtime->quiet << errorMessage << " " << str1 << endl;
	fail_exit(1);
}

void NC_Error_Warning_1 
(
	const string& errorMessage,
	const string& str1,
	const int& int1
)
{
	runtime->quiet << "\nWarning: " << errorMessage << endl;
	runtime->quiet << str1 << int1 << "\n" << endl;
}

void NC_Error_Warning_1 
(
	const string& errorMessage,
	const string& str1,
	const double& d1
)
{
	runtime->quiet << "\nWarning: " << errorMessage << endl;
	runtime->quiet << str1 << d1 << "\n" << endl;
}


void NC_Error_Exit_2 
(
	ofstream& NCout,
	const string& errorMessage,
	const string& str1,
	const int& int1,
	const string& str2,
	const int& int2
)
{
	runtime->quiet << errorMessage << endl;
	runtime->quiet << str1 << int1 << endl;
	runtime->quiet << str2 << int2 << endl;
	NCout << errorMessage << endl;
	NCout << str1 << int1 << endl;
	NCout << str2 << int2 << endl;
	fail_exit(1);
}

void NC_Error_Exit_2 
(
	ofstream& NCout,
	const string& errorMessage,
	const string& str1,
	const double& d1,
	const string& str2,
	const double& d2
)
{
	runtime->quiet << errorMessage << endl;
	runtime->quiet << str1 << d1 << endl;
	runtime->quiet << str2 << d2 << endl;
	NCout << errorMessage << endl;
	NCout << str1 << d1 << endl;
	NCout << str2 << d2 << endl;
	fail_exit(1);
}

void NC_Error_Warning_2 
(
	ofstream& NCout,
	const string& errorMessage,
	const string& str1,
	const int& int1,
	const string& str2,
	const int& int2
)
{
	runtime->quiet << "\nWarning: " << errorMessage << endl;
	runtime->quiet << str1 << int1 << endl;
	runtime->quiet << str2 << int2 << "\n" << endl;
	NCout << "\nWarning: " << errorMessage << endl;
	NCout << str1 << int1 << endl;
	NCout << str2 << int2 << "\n" << endl;
}

void NC_Error_Warning_2 
(
	ofstream& NCout,
	const string& errorMessage,
	const string& str1,
	const double& d1,
	const string& str2,
	const double& d2
)
{
	runtime->quiet << "\nWarning: " << errorMessage << endl;
	runtime->quiet << str1 << d1 << endl;
	runtime->quiet << str2 << d2 << "\n" << endl;
	NCout << "\nWarning: " << errorMessage << endl;
	NCout << str1 << d1 << endl;
	NCout << str2 << d2 << "\n" << endl;
}

void NC_Error_Warning_2 
(
	ofstream& NCout,
	const string& errorMessage,
	const string& str1,
	const int& i1,
	const string& str2,
	const double& d2
)
{
	runtime->quiet << "\nWarning: " << errorMessage << endl;
	runtime->quiet << str1 << i1 << endl;
	runtime->quiet << str2 << d2 << "\n" << endl;
	NCout << "\nWarning: " << errorMessage << endl;
	NCout << str1 << i1 << endl;
	NCout << str2 << d2 << "\n" << endl;
}


void NC_Error_Exit_2 
(
	const string& errorMessage,
	const string& str1,
	const int& int1,
	const string& str2,
	const int& int2
)
{
	runtime->quiet << errorMessage << endl;
	runtime->quiet << str1 << int1 << endl;
	runtime->quiet << str2 << int2 << endl;
	fail_exit(1);
}

void NC_Error_Exit_2 
(
	const string& errorMessage,
	const string& str1,
	const double& d1,
	const string& str2,
	const double& d2
)
{
	runtime->quiet << errorMessage << endl;
	runtime->quiet << str1 << d1 << endl;
	runtime->quiet << str2 << d2 << endl;
	fail_exit(1);
}

void NC_Error_Warning_2 
(
	const string& errorMessage,
	const string& str1,
	const int& int1,
	const string& str2,
	const int& int2
)
{
	runtime->quiet << "\nWarning: " << errorMessage << endl;
	runtime->quiet << str1 << int1 << endl;
	runtime->quiet << str2 << int2 << "\n" << endl;
}

void NC_Error_Warning_2 
(
	const string& errorMessage,
	const string& str1,
	const double& d1,
	const string& str2,
	const double& d2
)
{
	runtime->quiet << "\nWarning: " << errorMessage << endl;
	runtime->quiet << str1 << d1 << endl;
	runtime->quiet << str2 << d2 << "\n" << endl;
}


ostream& operator<<(ostream &s, const Complex &u)
{
    s << "(" << u.re() << ", " << u.im() << ")";
    return s;
};


double BLdispoi(Vector<double>& pointa, Vector<double>& pointb)
{
	double dist = 0.0, dif;
	int i;

	if(pointa.size() < NDIM || pointb.size() < NDIM)
		NC_Error_Exit_0(runtime->jobLog, "Dimension of the vectors must be not less than NDIM!");

	for(i=0; i<NDIM; i++)
	{ checkpoint(); 
		dif = pointa[i] - pointb[i];
		dist += dif*dif;
	}
	dist = sqrt(dist);

	return dist;
}


Complex BLzsqrt(const Complex& z)
{
	Complex zsq;
	double re1, ph1;

	if(z.norm() == 0.0)
	{
		zsq.set(0.0, 0.0);
	}
	else
	{
		re1 = sqrt(z.norm());
		ph1 = atan2(z.im(), z.re())/2.0;
		zsq.set(re1*cos(ph1), re1*sin(ph1));
	}

	return zsq;
}


void BLGauCooWe
(
	ofstream& NCout,
	const int& npt,          
	Vector<double>& crdgp,   
	Vector<double>& weigp    
)
{
	int i, hpt = npt/2, hp1 = hpt - 1;

	switch(npt)
	{
	case 1:
		crdgp[0] = GCoorWei_1[0][0];
		weigp[0] = GCoorWei_1[0][1];
		break;
	case 2:
		for(i=0; i<hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_2[i][0];
			crdgp[hp1 - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hp1 - i] = GCoorWei_2[i][1];
		}
		break;
	case 3:
		crdgp[hpt] = GCoorWei_3[0][0];
		weigp[hpt] = GCoorWei_3[0][1];
		for(i=1; i<=hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_3[i][0];
			crdgp[hpt - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hpt - i] = GCoorWei_3[i][1];
		}
		break;
	case 4:
		for(i=0; i<hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_4[i][0];
			crdgp[hp1 - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hp1 - i] = GCoorWei_4[i][1];
		}
		break;
	case 5:
		crdgp[hpt] = GCoorWei_5[0][0];
		weigp[hpt] = GCoorWei_5[0][1];
		for(i=1; i<=hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_5[i][0];
			crdgp[hpt - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hpt - i] = GCoorWei_5[i][1];
		}
		break;
	case 6:
		for(i=0; i<hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_6[i][0];
			crdgp[hp1 - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hp1 - i] = GCoorWei_6[i][1];
		}
		break;
	case 7:
		crdgp[hpt] = GCoorWei_7[0][0];
		weigp[hpt] = GCoorWei_7[0][1];
		for(i=1; i<=hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_7[i][0];
			crdgp[hpt - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hpt - i] = GCoorWei_7[i][1];
		}
		break;
	case 8:
		for(i=0; i<hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_8[i][0];
			crdgp[hp1 - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hp1 - i] = GCoorWei_8[i][1];
		}
		break;
	case 9:
		crdgp[hpt] = GCoorWei_9[0][0];
		weigp[hpt] = GCoorWei_9[0][1];
		for(i=1; i<=hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_9[i][0];
			crdgp[hpt - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hpt - i] = GCoorWei_9[i][1];
		}
		break;
	case 10:
		for(i=0; i<hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_10[i][0];
			crdgp[hp1 - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hp1 - i] = GCoorWei_10[i][1];
		}
		break;
	case 11:
		crdgp[hpt] = GCoorWei_11[0][0];
		weigp[hpt] = GCoorWei_11[0][1];
		for(i=1; i<=hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_11[i][0];
			crdgp[hpt - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hpt - i] = GCoorWei_11[i][1];
		}
		break;
	case 12:
		for(i=0; i<hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_12[i][0];
			crdgp[hp1 - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hp1 - i] = GCoorWei_12[i][1];
		}
		break;
	case 13:
		crdgp[hpt] = GCoorWei_13[0][0];
		weigp[hpt] = GCoorWei_13[0][1];
		for(i=1; i<=hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_13[i][0];
			crdgp[hpt - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hpt - i] = GCoorWei_13[i][1];
		}
		break;
	case 14:
		for(i=0; i<hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_14[i][0];
			crdgp[hp1 - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hp1 - i] = GCoorWei_14[i][1];
		}
		break;
	case 15:
		crdgp[hpt] = GCoorWei_15[0][0];
		weigp[hpt] = GCoorWei_15[0][1];
		for(i=1; i<=hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_15[i][0];
			crdgp[hpt - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hpt - i] = GCoorWei_15[i][1];
		}
		break;
	case 16:
		for(i=0; i<hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_16[i][0];
			crdgp[hp1 - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hp1 - i] = GCoorWei_16[i][1];
		}
		break;
	case 17:
		crdgp[hpt] = GCoorWei_17[0][0];
		weigp[hpt] = GCoorWei_17[0][1];
		for(i=1; i<=hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_17[i][0];
			crdgp[hpt - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hpt - i] = GCoorWei_17[i][1];
		}
		break;
	case 18:
		for(i=0; i<hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_18[i][0];
			crdgp[hp1 - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hp1 - i] = GCoorWei_18[i][1];
		}
		break;
	case 19:
		crdgp[hpt] = GCoorWei_19[0][0];
		weigp[hpt] = GCoorWei_19[0][1];
		for(i=1; i<=hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_19[i][0];
			crdgp[hpt - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hpt - i] = GCoorWei_19[i][1];
		}
		break;
	case 20:
		for(i=0; i<hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_20[i][0];
			crdgp[hp1 - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hp1 - i] = GCoorWei_20[i][1];
		}
		break;
	case 21:
		crdgp[hpt] = GCoorWei_21[0][0];
		weigp[hpt] = GCoorWei_21[0][1];
		for(i=1; i<=hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_21[i][0];
			crdgp[hpt - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hpt - i] = GCoorWei_21[i][1];
		}
		break;
	case 22:
		for(i=0; i<hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_22[i][0];
			crdgp[hp1 - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hp1 - i] = GCoorWei_22[i][1];
		}
		break;
	case 23:
		crdgp[hpt] = GCoorWei_23[0][0];
		weigp[hpt] = GCoorWei_23[0][1];
		for(i=1; i<=hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_23[i][0];
			crdgp[hpt - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hpt - i] = GCoorWei_23[i][1];
		}
		break;
	case 24:
		for(i=0; i<hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_24[i][0];
			crdgp[hp1 - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hp1 - i] = GCoorWei_24[i][1];
		}
		break;
	case 25:
		crdgp[hpt] = GCoorWei_25[0][0];
		weigp[hpt] = GCoorWei_25[0][1];
		for(i=1; i<=hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_25[i][0];
			crdgp[hpt - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hpt - i] = GCoorWei_25[i][1];
		}
		break;
	case 26:
		for(i=0; i<hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_26[i][0];
			crdgp[hp1 - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hp1 - i] = GCoorWei_26[i][1];
		}
		break;
	case 27:
		crdgp[hpt] = GCoorWei_27[0][0];
		weigp[hpt] = GCoorWei_27[0][1];
		for(i=1; i<=hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_27[i][0];
			crdgp[hpt - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hpt - i] = GCoorWei_27[i][1];
		}
		break;
	case 28:
		for(i=0; i<hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_28[i][0];
			crdgp[hp1 - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hp1 - i] = GCoorWei_28[i][1];
		}
		break;
	case 29:
		crdgp[hpt] = GCoorWei_29[0][0];
		weigp[hpt] = GCoorWei_29[0][1];
		for(i=1; i<=hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_29[i][0];
			crdgp[hpt - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hpt - i] = GCoorWei_29[i][1];
		}
		break;
	case 30:
		for(i=0; i<hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_30[i][0];
			crdgp[hp1 - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hp1 - i] = GCoorWei_30[i][1];
		}
		break;
	case 31:
		crdgp[hpt] = GCoorWei_31[0][0];
		weigp[hpt] = GCoorWei_31[0][1];
		for(i=1; i<=hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_31[i][0];
			crdgp[hpt - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hpt - i] = GCoorWei_31[i][1];
		}
		break;
	case 32:
		for(i=0; i<hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_32[i][0];
			crdgp[hp1 - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hp1 - i] = GCoorWei_32[i][1];
		}
		break;
	case 33:
		crdgp[hpt] = GCoorWei_33[0][0];
		weigp[hpt] = GCoorWei_33[0][1];
		for(i=1; i<=hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_33[i][0];
			crdgp[hpt - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hpt - i] = GCoorWei_33[i][1];
		}
		break;
	case 34:
		for(i=0; i<hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_34[i][0];
			crdgp[hp1 - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hp1 - i] = GCoorWei_34[i][1];
		}
		break;
	case 35:
		crdgp[hpt] = GCoorWei_35[0][0];
		weigp[hpt] = GCoorWei_35[0][1];
		for(i=1; i<=hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_35[i][0];
			crdgp[hpt - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hpt - i] = GCoorWei_35[i][1];
		}
		break;
	case 36:
		for(i=0; i<hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_36[i][0];
			crdgp[hp1 - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hp1 - i] = GCoorWei_36[i][1];
		}
		break;
	case 37:
		crdgp[hpt] = GCoorWei_37[0][0];
		weigp[hpt] = GCoorWei_37[0][1];
		for(i=1; i<=hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_37[i][0];
			crdgp[hpt - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hpt - i] = GCoorWei_37[i][1];
		}
		break;
	case 38:
		for(i=0; i<hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_38[i][0];
			crdgp[hp1 - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hp1 - i] = GCoorWei_38[i][1];
		}
		break;
	case 39:
		crdgp[hpt] = GCoorWei_39[0][0];
		weigp[hpt] = GCoorWei_39[0][1];
		for(i=1; i<=hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_39[i][0];
			crdgp[hpt - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hpt - i] = GCoorWei_39[i][1];
		}
		break;
	case 40:
		for(i=0; i<hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_40[i][0];
			crdgp[hp1 - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hp1 - i] = GCoorWei_40[i][1];
		}
		break;
	case 41:
		crdgp[hpt] = GCoorWei_41[0][0];
		weigp[hpt] = GCoorWei_41[0][1];
		for(i=1; i<=hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_41[i][0];
			crdgp[hpt - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hpt - i] = GCoorWei_41[i][1];
		}
		break;
	case 42:
		for(i=0; i<hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_42[i][0];
			crdgp[hp1 - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hp1 - i] = GCoorWei_42[i][1];
		}
		break;
	case 43:
		crdgp[hpt] = GCoorWei_43[0][0];
		weigp[hpt] = GCoorWei_43[0][1];
		for(i=1; i<=hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_43[i][0];
			crdgp[hpt - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hpt - i] = GCoorWei_43[i][1];
		}
		break;
	case 44:
		for(i=0; i<hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_44[i][0];
			crdgp[hp1 - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hp1 - i] = GCoorWei_44[i][1];
		}
		break;
	case 45:
		crdgp[hpt] = GCoorWei_45[0][0];
		weigp[hpt] = GCoorWei_45[0][1];
		for(i=1; i<=hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_45[i][0];
			crdgp[hpt - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hpt - i] = GCoorWei_45[i][1];
		}
		break;
	case 46:
		for(i=0; i<hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_46[i][0];
			crdgp[hp1 - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hp1 - i] = GCoorWei_46[i][1];
		}
		break;
	case 47:
		crdgp[hpt] = GCoorWei_47[0][0];
		weigp[hpt] = GCoorWei_47[0][1];
		for(i=1; i<=hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_47[i][0];
			crdgp[hpt - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hpt - i] = GCoorWei_47[i][1];
		}
		break;
	case 48:
		for(i=0; i<hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_48[i][0];
			crdgp[hp1 - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hp1 - i] = GCoorWei_48[i][1];
		}
		break;
	case 49:
		crdgp[hpt] = GCoorWei_49[0][0];
		weigp[hpt] = GCoorWei_49[0][1];
		for(i=1; i<=hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_49[i][0];
			crdgp[hpt - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hpt - i] = GCoorWei_49[i][1];
		}
		break;
	case 50:
		for(i=0; i<hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_50[i][0];
			crdgp[hp1 - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hp1 - i] = GCoorWei_50[i][1];
		}
		break;
	case 51:
		crdgp[hpt] = GCoorWei_51[0][0];
		weigp[hpt] = GCoorWei_51[0][1];
		for(i=1; i<=hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_51[i][0];
			crdgp[hpt - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hpt - i] = GCoorWei_51[i][1];
		}
		break;
	case 52:
		for(i=0; i<hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_52[i][0];
			crdgp[hp1 - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hp1 - i] = GCoorWei_52[i][1];
		}
		break;
	case 53:
		crdgp[hpt] = GCoorWei_53[0][0];
		weigp[hpt] = GCoorWei_53[0][1];
		for(i=1; i<=hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_53[i][0];
			crdgp[hpt - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hpt - i] = GCoorWei_53[i][1];
		}
		break;
	case 54:
		for(i=0; i<hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_54[i][0];
			crdgp[hp1 - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hp1 - i] = GCoorWei_54[i][1];
		}
		break;
	case 55:
		crdgp[hpt] = GCoorWei_55[0][0];
		weigp[hpt] = GCoorWei_55[0][1];
		for(i=1; i<=hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_55[i][0];
			crdgp[hpt - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hpt - i] = GCoorWei_55[i][1];
		}
		break;
	case 56:
		for(i=0; i<hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_56[i][0];
			crdgp[hp1 - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hp1 - i] = GCoorWei_56[i][1];
		}
		break;
	case 57:
		crdgp[hpt] = GCoorWei_57[0][0];
		weigp[hpt] = GCoorWei_57[0][1];
		for(i=1; i<=hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_57[i][0];
			crdgp[hpt - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hpt - i] = GCoorWei_57[i][1];
		}
		break;
	case 58:
		for(i=0; i<hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_58[i][0];
			crdgp[hp1 - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hp1 - i] = GCoorWei_58[i][1];
		}
		break;
	case 59:
		crdgp[hpt] = GCoorWei_59[0][0];
		weigp[hpt] = GCoorWei_59[0][1];
		for(i=1; i<=hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_59[i][0];
			crdgp[hpt - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hpt - i] = GCoorWei_59[i][1];
		}
		break;
	case 60:
		for(i=0; i<hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_60[i][0];
			crdgp[hp1 - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hp1 - i] = GCoorWei_60[i][1];
		}
		break;
	case 61:
		crdgp[hpt] = GCoorWei_61[0][0];
		weigp[hpt] = GCoorWei_61[0][1];
		for(i=1; i<=hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_61[i][0];
			crdgp[hpt - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hpt - i] = GCoorWei_61[i][1];
		}
		break;
	case 62:
		for(i=0; i<hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_62[i][0];
			crdgp[hp1 - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hp1 - i] = GCoorWei_62[i][1];
		}
		break;
	case 63:
		crdgp[hpt] = GCoorWei_63[0][0];
		weigp[hpt] = GCoorWei_63[0][1];
		for(i=1; i<=hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_63[i][0];
			crdgp[hpt - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hpt - i] = GCoorWei_63[i][1];
		}
		break;
	case 64:
		for(i=0; i<hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_64[i][0];
			crdgp[hp1 - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hp1 - i] = GCoorWei_64[i][1];
		}
		break;
	case 65:
		crdgp[hpt] = GCoorWei_65[0][0];
		weigp[hpt] = GCoorWei_65[0][1];
		for(i=1; i<=hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_65[i][0];
			crdgp[hpt - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hpt - i] = GCoorWei_65[i][1];
		}
		break;
	case 66:
		for(i=0; i<hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_66[i][0];
			crdgp[hp1 - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hp1 - i] = GCoorWei_66[i][1];
		}
		break;
	case 67:
		crdgp[hpt] = GCoorWei_67[0][0];
		weigp[hpt] = GCoorWei_67[0][1];
		for(i=1; i<=hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_67[i][0];
			crdgp[hpt - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hpt - i] = GCoorWei_67[i][1];
		}
		break;
	case 68:
		for(i=0; i<hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_68[i][0];
			crdgp[hp1 - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hp1 - i] = GCoorWei_68[i][1];
		}
		break;
	case 69:
		crdgp[hpt] = GCoorWei_69[0][0];
		weigp[hpt] = GCoorWei_69[0][1];
		for(i=1; i<=hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_69[i][0];
			crdgp[hpt - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hpt - i] = GCoorWei_69[i][1];
		}
		break;
	case 70:
		for(i=0; i<hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_70[i][0];
			crdgp[hp1 - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hp1 - i] = GCoorWei_70[i][1];
		}
		break;
	case 71:
		crdgp[hpt] = GCoorWei_71[0][0];
		weigp[hpt] = GCoorWei_71[0][1];
		for(i=1; i<=hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_71[i][0];
			crdgp[hpt - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hpt - i] = GCoorWei_71[i][1];
		}
		break;
	case 72:
		for(i=0; i<hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_72[i][0];
			crdgp[hp1 - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hp1 - i] = GCoorWei_72[i][1];
		}
		break;
	case 73:
		crdgp[hpt] = GCoorWei_73[0][0];
		weigp[hpt] = GCoorWei_73[0][1];
		for(i=1; i<=hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_73[i][0];
			crdgp[hpt - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hpt - i] = GCoorWei_73[i][1];
		}
		break;
	case 74:
		for(i=0; i<hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_74[i][0];
			crdgp[hp1 - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hp1 - i] = GCoorWei_74[i][1];
		}
		break;
	case 75:
		crdgp[hpt] = GCoorWei_75[0][0];
		weigp[hpt] = GCoorWei_75[0][1];
		for(i=1; i<=hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_75[i][0];
			crdgp[hpt - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hpt - i] = GCoorWei_75[i][1];
		}
		break;
	case 76:
		for(i=0; i<hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_76[i][0];
			crdgp[hp1 - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hp1 - i] = GCoorWei_76[i][1];
		}
		break;
	case 77:
		crdgp[hpt] = GCoorWei_77[0][0];
		weigp[hpt] = GCoorWei_77[0][1];
		for(i=1; i<=hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_77[i][0];
			crdgp[hpt - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hpt - i] = GCoorWei_77[i][1];
		}
		break;
	case 78:
		for(i=0; i<hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_78[i][0];
			crdgp[hp1 - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hp1 - i] = GCoorWei_78[i][1];
		}
		break;
	case 79:
		crdgp[hpt] = GCoorWei_79[0][0];
		weigp[hpt] = GCoorWei_79[0][1];
		for(i=1; i<=hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_79[i][0];
			crdgp[hpt - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hpt - i] = GCoorWei_79[i][1];
		}
		break;
	case 80:
		for(i=0; i<hpt; i++) { checkpoint(); 
			crdgp[hpt + i] = GCoorWei_80[i][0];
			crdgp[hp1 - i] = -crdgp[hpt + i];
			weigp[hpt + i] = weigp[hp1 - i] = GCoorWei_80[i][1];
		}
		break;
	default:
		NC_Error_Exit_1(NCout, "Number of Gaussean points must be in the interval [1, 80]!",
			"Number of Gaussean points = ", npt);
	}
}


void NC_IterativeSolverCGS
(
	ofstream& NCout
)
{
	Vector<Complex> zX_j(state().numRowsOfCoefficientMatrix_), zR_0(state().numRowsOfCoefficientMatrix_), zR_j(state().numRowsOfCoefficientMatrix_), zP_j(state().numRowsOfCoefficientMatrix_), zV_j(state().numRowsOfCoefficientMatrix_),
		zQ_j(state().numRowsOfCoefficientMatrix_), zU_j(state().numRowsOfCoefficientMatrix_), zUQ_j(state().numRowsOfCoefficientMatrix_), zAUQ_j(state().numRowsOfCoefficientMatrix_);
	int i, j;
	double err_ori = 0.0, err_rel = 1.0, dwk1;
	Complex zalph, zbet, zrjr0, zrjr1;
	bool ifmodyprecond = false;

BeginCGS:
	
	switch(state().methodPreconditioner_)
	{
	case 0: 
		NC_IncompleteLUDecomposition(NCout);
		break;
	case 1: 
		NC_ComputeScalingVector(NCout);
		break;
	}


	
	for(j=0; j<state().numRowsOfCoefficientMatrix_; j++)
	{ checkpoint(); 
	  
          
	  zX_j[j].set(0.0,0.0);
	}

	NC_MatrixVectorMultiplication(zX_j, zV_j);

	
	if(!state().methodPreconditioner_)
	{
		for(i=0; i<state().numRowsOfCoefficientMatrix_; i++)
		{ checkpoint(); 
			if(zV_j[i].norm() > 1.0e8)
			{
				if(state().methodFMM_)
				{
					ifmodyprecond = true;
					state().methodPreconditioner_ = 1;
					NC_Error_Warning_0(NCout,
					"ILU preconditioning failed, the row scanning method is used!");
					goto BeginCGS;
				}
				else
				{
					goto DirectCGS;
				}
			}
		}
	}

	for(i=0; i<state().numRowsOfCoefficientMatrix_; i++) zU_j[i] = zR_j[i] = zP_j[i] = zR_0[i] = state().zrhs[i] - zV_j[i];

	for(j=0; j<=state().niter_max_; j++)
	{ checkpoint(); 
		NC_MatrixVectorMultiplication(zP_j, zV_j);

		if(j==0) {zrjr0 = zR_j*zR_0;} else {zrjr0 = zrjr1;}
		zalph = zrjr0/(zV_j*zR_0);

		for(i=0; i<state().numRowsOfCoefficientMatrix_; i++) zQ_j[i] = zU_j[i] - zalph*zV_j[i];
		zUQ_j = zU_j + zQ_j;

		NC_MatrixVectorMultiplication(zUQ_j, zAUQ_j);

		dwk1 = 0;
		for(i=0; i<state().numRowsOfCoefficientMatrix_; i++)
		{ checkpoint(); 
		  zX_j[i] += zalph*zUQ_j[i];
		  zR_j[i] -= zalph*zAUQ_j[i];
		  
		  dwk1 += zR_j[i].qnorm();
		}

		if(j == 0)
		{
			err_ori = sqrt(dwk1);
			runtime->quiet << "\nCGS: err_ori = " << err_ori << endl;
		}
		else
		{
            err_rel = sqrt(dwk1)/err_ori;
		}

		if(j > 0 && j/10*10 == j) {
		  runtime->quiet << j << " " << err_rel << endl;
		  runtime->quiet << j << " Abs Error:" << sqrt(dwk1) << endl;
		}

		if(runtime->compactFar && (!std::isfinite(err_rel) || (j==state().niter_max_ && err_rel>=state().ErroIterSols)))throw Failure(1,"Bounded ML-FMM did not converge");
        if(err_rel < state().ErroIterSols || j == state().niter_max_)
		{
			if(j/10*10 != j) runtime->quiet << j << " " << err_rel << "\n" << endl;
			for(i=0; i<state().numRowsOfCoefficientMatrix_; i++) state().zrhs[i] = zX_j[i];
			if(j == state().niter_max_) {
				NC_Error_Warning_0(NCout, "Warning: Maximum number of iterations is reached!");
			}
			break;
		}

		zrjr1 = zR_j*zR_0;
		zbet = zrjr1/zrjr0;
		for(i=0; i<state().numRowsOfCoefficientMatrix_; i++) zU_j[i] = zR_j[i] + zbet*zQ_j[i];
		for(i=0; i<state().numRowsOfCoefficientMatrix_; i++) zP_j[i] = zU_j[i] + zbet*(zQ_j[i] + zbet*zP_j[i]);
	} 

	NCout << "\nCGS solver: number of iterations = "
		<< j << ", relative error = " << err_rel << endl;

	if(j >= state().niter_max_ && state().methodFMM_ == 0)
	{
DirectCGS:
		NC_Error_Warning_0(NCout, "Iteration method CGS failed, direct method is used!");

		
		Tfactor_usy(state().zcoefl, state().numRowsOfCoefficientMatrix_);
		Tfbelim(state().zcoefl, state().zrhs, state().numRowsOfCoefficientMatrix_);

		return;
	}

	
	switch(state().methodPreconditioner_)
	{
	case 0: 
		NC_IncompleteLUForBack();
		NC_DeleteIncompleteLUMatrices();
		break;
	case 1: 
        for(i=0; i<state().numRowsOfCoefficientMatrix_; i++) state().zrhs[i] *= state().dscaling[i];
		release_array(state().dscaling);
		break;
	}

	if(ifmodyprecond) state().methodPreconditioner_ = 0;

}


void NC_MatrixVectorMultiplication
(
	Vector<Complex>& zmultvect,		
	Vector<Complex>& zresuvect      
)
{
	int i, j, k, ilv;
	Vector<Complex> ztmp(state().numRowsOfCoefficientMatrix_);

	
	switch(state().methodPreconditioner_)
	{
	case 0: 
		NC_IncompleteLUForBack(runtime->jobLog, zmultvect, ztmp);
		break;
	case 1: 
		for(i=0; i<state().numRowsOfCoefficientMatrix_; i++) ztmp[i] = zmultvect[i]*state().dscaling[i];
		break;
	case 2: 
		ztmp = zmultvect;
		break;
	}

	
	switch(state().methodFMM_)
	{
	case 0: 
		k = 0;
		for(i=0; i<state().numRowsOfCoefficientMatrix_; i++)
		{ checkpoint(); 
			zresuvect[i].set(0.0, 0.0);
			for(j=0; j<state().numRowsOfCoefficientMatrix_; j++) zresuvect[i] += state().zcoefl[k + j]*ztmp[j];
			k += state().numRowsOfCoefficientMatrix_;
		}
		break;

	case 1: 
		{
			Vector<Complex> zvc_t(state().numIntegrationPointsUnitSphere_*state().numOriginalReflectedClusters_), zvc_d(state().dmtxlev[0].nRowsD);

			for(i=0; i<state().numIntegrationPointsUnitSphere_*state().numOriginalReflectedClusters_; i++)
			{ checkpoint(); 
				zvc_t[i].set(0.0, 0.0);
				for(j=state().irowtmtx[i]; j<state().irowtmtx[i + 1]; j++)
					zvc_t[i] += state().ztmtx[j]*ztmp[state().jcoltmtx[(j-state().irowtmtx[i])+state().irowtmtx[i-i%state().numIntegrationPointsUnitSphere_]/state().numIntegrationPointsUnitSphere_]];
			}

			for(i=0; i<state().dmtxlev[0].nRowsD; i++)
			{ checkpoint(); 
				zvc_d[i].set(0.0, 0.0);
				for(j=state().dmtxlev[0].irowDmxLv[i]; j<state().dmtxlev[0].irowDmxLv[i + 1]; j++)
					zvc_d[i] += state().dmtxlev[0].zDmxLv[j]*zvc_t[(state().clustarry[i/state().numIntegrationPointsUnitSphere_].NumsFarClus[j-state().dmtxlev[0].irowDmxLv[i]]*state().numIntegrationPointsUnitSphere_+i%state().numIntegrationPointsUnitSphere_)];
			}

			for(i=0; i<state().numRowsOfCoefficientMatrix_; i++)
			{ checkpoint(); 
				zresuvect[i].set(0.0, 0.0);

				
				for(j=state().irownea[i]; j<state().irownea[i + 1]; j++)
					zresuvect[i] += state().zcoefl[j]*ztmp[state().jcolnea[j+state().nearColumnBias[i]]];

				
				for(j=state().irowsmtx[i]; j<state().irowsmtx[i + 1]; j++)
					zresuvect[i] += state().zsmtx[j]*zvc_d[(j+state().jcolsmtx[i])];
			}
		}
		break;
	case 3: 
		
		for(i=0; i<state().numRowsOfCoefficientMatrix_; i++)
		{ checkpoint(); 
			zresuvect[i].set(0.0, 0.0);
			for(j=state().irownea[i]; j<state().irownea[i + 1]; j++)
				zresuvect[i] += state().zcoefl[j]*ztmp[state().jcolnea[j+state().nearColumnBias[i]]];
		}

		
		for(ilv=0; ilv<state().numClusterLevels_; ilv++)
		{ checkpoint(); 
			
			if(runtime->compactFar){packed_apply(&ztmp,&zresuvect);break;}
			for(i=0; i<state().clulevarry[ilv].nPoinSpheLv*state().clulevarry[ilv].nClustSLv; i++)
			{ checkpoint(); 
				state().clulevarry[ilv].zwkT[i].set(0.0, 0.0);
				for(j=state().tmtxlev[ilv].irowTmxLv[i]; j<state().tmtxlev[ilv].irowTmxLv[i + 1]; j++)
					state().clulevarry[ilv].zwkT[i] +=
					state().tmtxlev[ilv].zTmxLv[j]*ztmp[state().tmtxlev[ilv].jcolTmxLv[j]];
			}

			
			for(i=0; i<state().dmtxlev[ilv].nRowsD; i++)
			{ checkpoint(); 
				state().clulevarry[ilv].zwkS[i].set(0.0, 0.0);
				for(j=state().dmtxlev[ilv].irowDmxLv[i]; j<state().dmtxlev[ilv].irowDmxLv[i+1]; j++)
					state().clulevarry[ilv].zwkS[i] +=
					state().dmtxlev[ilv].zDmxLv[j]*state().clulevarry[ilv].zwkT[state().dmtxlev[ilv].jcolDmxLv[j]];
			}

			
			for(i=0; i<state().numRowsOfCoefficientMatrix_; i++)
			{ checkpoint(); 
				for(j=state().smtxlev[ilv].irowSmxLv[i]; j<state().smtxlev[ilv].irowSmxLv[i + 1]; j++)
					zresuvect[i] +=
					state().smtxlev[ilv].zSmxLv[j]*state().clulevarry[ilv].zwkS[state().smtxlev[ilv].jcolSmxLv[j]];
			}
		} 
		break;
	}
}


void NC_ComputeScalingVector
(
	ofstream& NCout
)
{
	int i, j, k;
	double dwk;

	state().dscaling = allocate_array<double>((state().numRowsOfCoefficientMatrix_));

	switch(state().methodFMM_)
	{
	case 0: 
		k = 0;
		for(i=0; i<state().numRowsOfCoefficientMatrix_; i++)
		{ checkpoint(); 
			dwk = 0.0;
			for(j=0; j<state().numRowsOfCoefficientMatrix_; j++) dwk += state().zcoefl[k + j].qnorm();
			state().dscaling[i] = sqrt((double)(state().numRowsOfCoefficientMatrix_)/dwk);
			k += state().numRowsOfCoefficientMatrix_;
		}
		break;
	case 1: 
	case 3: 
		for(i=0; i<state().numRowsOfCoefficientMatrix_; i++)
		{ checkpoint(); 
			dwk = 0.0;
			for(j=state().irownea[i]; j<state().irownea[i+1]; j++) dwk += state().zcoefl[j].qnorm();
			state().dscaling[i] = sqrt((double)(state().irownea[i+1] - state().irownea[i])/dwk);
		}
		break;
	}
}


void NC_IncompleteLUDecomposition
(
	ofstream& NCout
)
{
	int i, j, k, kl, ku, m, j1 = 0, ml, mu, n_trues = 0, ilv;
	int nunzL, nunzU;
	double threshfac; 
	double dwk, dw1;
	Vector<bool> Mirow(state().numRowsOfCoefficientMatrix_), Micol(state().numRowsOfCoefficientMatrix_), Mkvct(state().numRowsOfCoefficientMatrix_);

	if(state().methodFMM_ == 0) { 
		if(state().scanningDegreeLU_ == 0) {
			threshfac = 1.2;
		} else if(state().scanningDegreeLU_ == 1) {
			threshfac = 1.0;
		} else if(state().scanningDegreeLU_ == 2) {
			threshfac = 0.8;
		} else {
			threshfac = 0.6;
		}
	} else if(state().methodFMM_ == 1) { 
		if(state().scanningDegreeLU_ == 0) {
			
			threshfac = 0.9;
		} else if(state().scanningDegreeLU_ == 1) {
			
			threshfac = 0.35;
		} else if(state().scanningDegreeLU_ == 2) {
			
			threshfac = 0.07;
		} else {
			threshfac = 0.01;
		}
	} else { 
		if(state().scanningDegreeLU_ == 0) {
			
			threshfac = 0.65;
		} else if(state().scanningDegreeLU_ == 1) {
			
			threshfac = 0.15;
		} else if(state().scanningDegreeLU_ == 2) {
			
			threshfac = 0.05;
		} else {
			threshfac = 0.005;
		}
	}

	switch(state().methodFMM_)
	{
	case 0: 
		
		k = 0;
		for(i=0; i<state().numRowsOfCoefficientMatrix_; i++)
		{ checkpoint(); 
			dw1 = 0.0;
			for(j=0; j<state().numRowsOfCoefficientMatrix_; j++) dw1 += state().zcoefl[k + j].qnorm();
			dwk = sqrt((double)(state().numRowsOfCoefficientMatrix_)/dw1);
			for(j=0; j<state().numRowsOfCoefficientMatrix_; j++)
			{ checkpoint(); 
				state().zcoefl[k + j] *= dwk;
				if(state().zcoefl[k + j].norm() > threshfac || i == j) n_trues++;
			}
			state().zrhs[i] *= dwk;
			k += state().numRowsOfCoefficientMatrix_;
		} 
		break;
	case 1: 
		
		for(i=0; i<state().numRowsOfCoefficientMatrix_; i++)
		{ checkpoint(); 
			dw1 = 0.0;
			for(j=state().irownea[i]; j<state().irownea[i + 1]; j++) dw1 += state().zcoefl[j].qnorm();
			dwk = sqrt((double)(state().irownea[i + 1] - state().irownea[i])/dw1);
			for(j=state().irownea[i]; j<state().irownea[i + 1]; j++)
			{ checkpoint(); 
				state().zcoefl[j] *= dwk;
				if(state().zcoefl[j].norm() > threshfac || state().jcolnea[j+state().nearColumnBias[i]] == i) n_trues++;
			}
			for(j=state().irowsmtx[i]; j<state().irowsmtx[i + 1]; j++) state().zsmtx[j] *= dwk;
			state().zrhs[i] *= dwk;
		}
		break;
	case 3: 
		
		for(i=0; i<state().numRowsOfCoefficientMatrix_; i++)
		{ checkpoint(); 
			dw1 = 0.0;
			for(j=state().irownea[i]; j<state().irownea[i + 1]; j++) dw1 += state().zcoefl[j].qnorm();
			dwk = sqrt((double)(state().irownea[i + 1] - state().irownea[i])/dw1);
			for(j=state().irownea[i]; j<state().irownea[i + 1]; j++)
			{ checkpoint(); 
				state().zcoefl[j] *= dwk;
				if(state().zcoefl[j].norm() > threshfac || state().jcolnea[j+state().nearColumnBias[i]] == i) n_trues++;
			}

			for(ilv=0; ilv<state().numClusterLevels_; ilv++)
			{ checkpoint(); 
				if(runtime->compactFar){if(ilv==0)runtime->packedRowScale[i]*=dwk;}
                else for(j=state().smtxlev[ilv].irowSmxLv[i]; j<state().smtxlev[ilv].irowSmxLv[i + 1]; j++)state().smtxlev[ilv].zSmxLv[j] *= dwk;
			}

			state().zrhs[i] *= dwk;
		}
		break;
	}

	Vector<int> jcol_tru(n_trues), nrow_tru(state().numRowsOfCoefficientMatrix_ + 1), irow_tru(n_trues),
		ncol_tru(state().numRowsOfCoefficientMatrix_ + 1);

	
	switch(state().methodFMM_)
	{
	case 0: 
		kl = k = 0;
		for(i=0; i<state().numRowsOfCoefficientMatrix_; i++) 
		{ checkpoint(); 
			nrow_tru[i] = kl;
			for(j=0; j<state().numRowsOfCoefficientMatrix_; j++) 
			{ checkpoint(); 
				if(state().zcoefl[k + j].norm() > threshfac || i == j) jcol_tru[kl++] = j;
			}
			k += state().numRowsOfCoefficientMatrix_;
		}
		nrow_tru[state().numRowsOfCoefficientMatrix_] = kl;
		break;
	case 1: 
	case 3: 
		kl = 0;
		for(i=0; i<state().numRowsOfCoefficientMatrix_; i++) 
		{ checkpoint(); 
			nrow_tru[i] = kl;
			for(j=state().irownea[i]; j<state().irownea[i + 1]; j++) 
			{ checkpoint(); 
				if(state().zcoefl[j].norm() > threshfac || state().jcolnea[j+state().nearColumnBias[i]] == i)
					jcol_tru[kl++] = state().jcolnea[j+state().nearColumnBias[i]];
			}
		}
		nrow_tru[state().numRowsOfCoefficientMatrix_] = kl;
		break;
	}

	
	kl = 0;
	for(j=0; j<state().numRowsOfCoefficientMatrix_; j++) 
	{ checkpoint(); 
		ncol_tru[j] = kl;
		for(i=0; i<state().numRowsOfCoefficientMatrix_; i++) 
		{ checkpoint(); 
			for(k=nrow_tru[i]; k<nrow_tru[i+1]; k++)
			{ checkpoint(); 
				if(jcol_tru[k] == j)
				{
					irow_tru[kl++] = i;
					break;
				}
			}
		}
	}
	ncol_tru[state().numRowsOfCoefficientMatrix_] = kl;

	
	nunzL = nunzU = 0;
	for(i=0; i<state().numRowsOfCoefficientMatrix_; i++)
	{ checkpoint(); 
		for(j=0; j<=i; j++) Mirow[j] = Micol[j] = false;
		for(j=nrow_tru[i]; j<nrow_tru[i+1]; j++) Mirow[jcol_tru[j]] = true;
		for(j=ncol_tru[i]; j<ncol_tru[i+1]; j++) Micol[irow_tru[j]] = true;

		for(j=0; j<=i; j++) if(Mirow[j]) nunzL++;
		for(j=0; j<i; j++) if(Micol[j]) nunzU++;
	}
	state().nL_colnu = allocate_array<int>((nunzL));
	state().nL_firnu = allocate_array<int>((state().numRowsOfCoefficientMatrix_ + 1));
	state().nU_rownu = allocate_array<int>((nunzU));
	state().nU_firnu = allocate_array<int>((state().numRowsOfCoefficientMatrix_ + 1));
	state().zL_incom = allocate_array<Complex>((nunzL));
	state().zU_incom = allocate_array<Complex>((nunzU));

	kl = ku = 0;
	for(i=0; i<state().numRowsOfCoefficientMatrix_; i++)
	{ checkpoint(); 
		for(j=0; j<=i; j++) Mirow[j] = Micol[j] = false;
		for(j=nrow_tru[i]; j<nrow_tru[i+1]; j++) Mirow[jcol_tru[j]] = true;
		for(j=ncol_tru[i]; j<ncol_tru[i+1]; j++) Micol[irow_tru[j]] = true;

		state().nL_firnu[i] = kl;
		for(j=0; j<=i; j++) if(Mirow[j]) state().nL_colnu[kl++] = j;

		state().nU_firnu[i] = ku;
		for(j=0; j<i; j++) if(Micol[j]) state().nU_rownu[ku++] = j;
	}
	state().nL_firnu[state().numRowsOfCoefficientMatrix_] = kl;
	state().nU_firnu[state().numRowsOfCoefficientMatrix_] = ku;

	
	switch(state().methodFMM_)
	{
	case 0: 
		k = 0;
		for(i=0; i<state().numRowsOfCoefficientMatrix_; i++)
		{ checkpoint(); 
			for(j=state().nL_firnu[i]; j<state().nL_firnu[i + 1]; j++) state().zL_incom[j] = state().zcoefl[k + state().nL_colnu[j]];

			for(j=state().nU_firnu[i]; j<state().nU_firnu[i + 1]; j++)
				state().zU_incom[j] = state().zcoefl[state().nU_rownu[j]*state().numRowsOfCoefficientMatrix_ + i];
			k += state().numRowsOfCoefficientMatrix_;
		}
		break;
	case 1: 
	case 3: 
		for(i=0; i<state().numRowsOfCoefficientMatrix_; i++)
		{ checkpoint(); 
			for(j=state().nL_firnu[i]; j<state().nL_firnu[i + 1]; j++)
			{ checkpoint(); 
				for(k=state().irownea[i]; k<state().irownea[i + 1]; k++) if(state().jcolnea[k+state().nearColumnBias[i]] == state().nL_colnu[j])
				{
					kl = k;
					break;
				}
				state().zL_incom[j] = state().zcoefl[kl];
			}

			for(j=state().nU_firnu[i]; j<state().nU_firnu[i + 1]; j++)
			{ checkpoint(); 
				for(k=state().irownea[state().nU_rownu[j]]; k<state().irownea[state().nU_rownu[j] + 1]; k++)
				if(state().jcolnea[k+state().nearColumnBias[state().nU_rownu[j]]] == i)
				{
					ku = k;
					break;
				}
				state().zU_incom[j] = state().zcoefl[ku];
			}
		} 
		break;
	}

	
	for(i=0; i<state().numRowsOfCoefficientMatrix_; i++)
	{ checkpoint(); 
		for(j=0; j<state().numRowsOfCoefficientMatrix_; j++) Mirow[j] = Micol[j] = false;
		for(j=nrow_tru[i]; j<nrow_tru[i+1]; j++) Mirow[jcol_tru[j]] = true;
		for(j=ncol_tru[i]; j<ncol_tru[i+1]; j++) Micol[irow_tru[j]] = true;

		for(k=i; k<state().numRowsOfCoefficientMatrix_; k++)
		{ checkpoint(); 
			if(!Micol[k]) continue;

			for(j=state().nL_firnu[k]; j<state().nL_firnu[k + 1]; j++)
			{ checkpoint(); 
				if(state().nL_colnu[j] == i)
				{
					j1 = j;
					break;
				}
			}

			for(j=0; j<state().numRowsOfCoefficientMatrix_; j++) Mkvct[j] = false;
			for(j=nrow_tru[k]; j<nrow_tru[k+1]; j++) Mkvct[jcol_tru[j]] = true;

			ml = mu = 0;
			for(m=0; m<i; m++)
			{ checkpoint(); 
				if(Mkvct[m] && Micol[m])
				{
					state().zL_incom[j1] -= state().zL_incom[state().nL_firnu[k] + ml]*state().zU_incom[state().nU_firnu[i] + mu];
				}
				if(Mkvct[m]) ml++;
				if(Micol[m]) mu++;
			}
		}

		for(k=i+1; k<state().numRowsOfCoefficientMatrix_; k++)
		{ checkpoint(); 
			if(!Mirow[k]) continue;

			for(j=state().nU_firnu[k]; j<state().nU_firnu[k + 1]; j++)
			{ checkpoint(); 
				if(state().nU_rownu[j] == i)
				{
					j1 = j;
					break;
				}
			}

			for(j=0; j<state().numRowsOfCoefficientMatrix_; j++) Mkvct[j] = false;
			for(j=ncol_tru[k]; j<ncol_tru[k+1]; j++) Mkvct[irow_tru[j]] = true;

			ml = mu = 0;
			for(m=0; m<i; m++)
			{ checkpoint(); 
				if(Mirow[m] && Mkvct[m])
				{
					state().zU_incom[j1] -= state().zL_incom[state().nL_firnu[i] + ml]*state().zU_incom[state().nU_firnu[k] + mu];
				}
				if(Mirow[m]) ml++;
				if(Mkvct[m]) mu++;
			}
			state().zU_incom[j1] /= state().zL_incom[state().nL_firnu[i + 1] - 1];
		}
	} 
}


void NC_DeleteIncompleteLUMatrices()
{
	release_array(state().nL_colnu);
	release_array(state().nL_firnu);
	release_array(state().nU_rownu);
	release_array(state().nU_firnu);
	release_array(state().zL_incom);
	release_array(state().zU_incom);
}


void NC_IncompleteLUForBack
(
	ofstream& NCout,
	Vector<Complex>& zmultvect,		
	Vector<Complex>& zprodvect      
)
{
	int i, j, k;

	zprodvect = zmultvect;

	
	zprodvect[0] /= state().zL_incom[0];
	for(i=1; i<state().numRowsOfCoefficientMatrix_; i++)
	{ checkpoint(); 
		k = state().nL_firnu[i + 1] - 1;
		for(j=state().nL_firnu[i]; j<k; j++) zprodvect[i] -= state().zL_incom[j]*zprodvect[state().nL_colnu[j]];
		zprodvect[i] /= state().zL_incom[k];
	}

	
	for(i=state().numRowsOfCoefficientMatrix_-1; i>0; i--)
	{ checkpoint(); 
		for(j=state().nU_firnu[i]; j<state().nU_firnu[i+1]; j++)
			zprodvect[state().nU_rownu[j]] -= state().zU_incom[j]*zprodvect[i];
	}
}
void NC_IncompleteLUForBack
(
)
{
	int i, j, k;

	
	state().zrhs[0] /= state().zL_incom[0];
	for(i=1; i<state().numRowsOfCoefficientMatrix_; i++)
	{ checkpoint(); 
		k = state().nL_firnu[i + 1] - 1;
		for(j=state().nL_firnu[i]; j<k; j++) state().zrhs[i] -= state().zL_incom[j]*state().zrhs[state().nL_colnu[j]];
		state().zrhs[i] /= state().zL_incom[k];
	}

	
	for(i=state().numRowsOfCoefficientMatrix_-1; i>0; i--)
	{ checkpoint(); 
		for(j=state().nU_firnu[i]; j<state().nU_firnu[i+1]; j++)
			state().zrhs[state().nU_rownu[j]] -= state().zU_incom[j]*state().zrhs[i];
	}
}
