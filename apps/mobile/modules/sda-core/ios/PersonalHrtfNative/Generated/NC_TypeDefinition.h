



#ifndef NC_TypeDefinition_h
#define NC_TypeDefinition_h




using namespace std;                                                                





class Complex {

protected:
	double re_, im_;

public:

	
	Complex() 
	{
		re_ = im_ = 0.0;
	}
	Complex(double rel, double img) 
	{
		re_ = rel;
		im_ = img;
	}
    Complex(const Complex &c)
    {
        re_ = c.re();
        im_ = c.im();
    }

    ~Complex(){}

    
    double re() const 
    {
        return(re_);
    }
    double im() const 
    {
        return(im_);
    }
    void set(double x, double y) 
    {
        re_ = x;
        im_ = y;
    }
    void set(const Complex z) 
    {
        re_ = z.re();
        im_ = z.im();
    }
    void mul_c(const Complex& x, const Complex& y)
    {
        re_ = x.re()*y.re() - x.im()*y.im();
        im_ = x.im()*y.re() + x.re()*y.im();
    }
    void mul_c(const Complex& x)
    {
        double tmp = re_;
        re_ = tmp*x.re() - im_*x.im();
        im_ = tmp*x.im() + im_*x.re();
    }
    void mul_r(const double& x)
    {
        re_ *= x;
        im_ *= x;
    }
    void mul_i(const double& y)
    {
        double tmp = re_;
        re_ = -im_*y;
        im_ = tmp*y;
    }
    void div_c(const Complex& x, const Complex& y)
    {
        double dy = y.re()*y.re() + y.im()*y.im();
        re_ = (x.re()*y.re() + x.im()*y.im())/dy;
        im_ = (x.im()*y.re() - x.re()*y.im())/dy;
    }
    void div_c(const Complex& x)
    {
        double tmp = re_, dx = x.re()*x.re() + x.im()*x.im();
        re_ = (tmp*x.re() + im_*x.im())/dx;
        im_ = (im_*x.re() - tmp*x.im())/dx;
    }
    void div_r(const double& x)
    {
        re_ /= x;
        im_ /= x;
    }
    void div_i(const double& y)
    {
        double tmp = re_;
        re_ = im_/y;
        im_ = -tmp/y;
    }
    void nega()
    {
        re_ = -re_;
        im_ = -im_;
    }
    void conju()
    {
        im_ = -im_;
    }
    Complex conjugate() const 
    {
        Complex z(re_, -im_);
        return(z);
    }
    double norm() const 
    {
        return sqrt(re_*re_ + im_*im_);
    }
    double qnorm() const 
    {
        return (re_*re_ + im_*im_);
    }
    double argu() const 
    {
        return atan2(im_, re_);
    }
    Complex zsqrt() const
    {
        double r, theta;
        r = sqrt(re_*re_ + im_*im_);
        theta = atan2(im_, re_)*0.5;
        r = sqrt(r);

        Complex z(r*cos(theta), r*sin(theta));
        return(z);
    }
    Complex zexp() const
    {
        double r = exp(re_);
        Complex z(r*cos(im_), r*sin(im_));
        return(z);
    }
    Complex zlog() const
    {
        Complex z(0.5*log(re_*re_ + im_*im_), atan2(im_, re_));
        return(z);
    }
    Complex zinv() const
    {
        double r = re_*re_ + im_*im_;
        Complex z(re_/r, -im_/r);
        return(z);
    }
    Complex znega() const
    {
        Complex z(-re_, -im_);
        return(z);
    }

    
    Complex operator+(const Complex& x) const
    {
        Complex z(re_ + x.re(), im_ + x.im());
        return(z);
    }
    Complex operator+(const double& x) const
    {
        Complex z(re_ + x, im_);
        return(z);
    }
    Complex operator-(const Complex& x) const
    {
        Complex z(re_ - x.re(), im_ - x.im());
        return(z);
    }
    Complex operator-(const double& x) const
    {
        Complex z(re_ - x, im_);
        return(z);
    }
    Complex operator*(const Complex& x) const
    {
        Complex z(re_*x.re() - im_*x.im(), re_*x.im() + im_*x.re());
        return(z);
    }
    Complex operator*(const double& x) const
    {
        Complex z(re_*x, im_*x);
        return(z);
    }
    Complex operator/(const Complex& x) const
    {
        double tmp = x.re()*x.re() + x.im()*x.im();
        Complex z((re_*x.re() + im_*x.im())/tmp, (im_*x.re() - re_*x.im())/tmp);
        return(z); 
    }
    Complex operator/(const double& x) const
    {
        Complex z(re_/x, im_/x);
        return(z);
    }
    Complex& operator=(const int& i)
    {
        re_ = (double)i;
        im_ = 0.0; 
        return *this;
    }
    Complex& operator=(const double& d)
    {
        re_ = d;
        im_ = 0.0; 
        return *this;
    }
    Complex& operator=(const Complex& x)
    {
        re_ = x.re();
        im_ = x.im(); 
        return *this;
    }
    void operator+=(const Complex& x)
    {
        re_ += x.re(); 
        im_ += x.im(); 
    }
    void operator+=(const double& x)
    {
        re_ += x;  
    }
    void operator-=(const Complex& x)
    {
        re_ -= x.re(); 
        im_ -= x.im(); 
    }
    void operator-=(const double& x)
    {
        re_ -= x;  
    }
    void operator*=(const Complex& x)
    {
        double tmpr = re_;
        re_ = tmpr*x.re() - im_*x.im(); 
        im_ = tmpr*x.im() + im_*x.re(); 
    }
    void operator*=(const double& x)
    {
        re_ *= x; 
        im_ *= x; 
    }
    void operator/=(const Complex& x)
    {
        double tmpr = re_, tmp = x.re()*x.re() + x.im()*x.im();
        re_ = (tmpr*x.re() + im_*x.im())/tmp; 
        im_ = (im_*x.re() - tmpr*x.im())/tmp; 
    }
    void operator/=(const double& x)
    {
        re_ /= x; 
        im_ /= x; 
    }
};


template <class T>
class Vector {

protected:

    int dim_;
    T *p_;

    
    void Init(int n)
    {
        p_ = n > 0 ? allocate_array<T>((n)) : NULL;
        dim_ = p_ != NULL ? n : 0;
    }
    void Reset()
    {
        if(p_ != NULL) release_array(p_);
        dim_ = 0;
        p_ = NULL;
    }

public:
    
    
    Vector() {
        Init(0);
    }
    Vector(int n)
    {
        Init(n);
    }
    Vector(int n, const T d)
    {
        Init(n);
        for(int i=0; i<dim_;) p_[i++] = d;
    }
    Vector(const Vector &B, int n) {
        Init(n);
        for(int i=0; i<B.size() && i<dim_; i++) p_[i] = B[i];
    }
    Vector(const Vector &B) {
        Init(B.size());
        for(int i=0; i<B.size(); i++) p_[i] = B[i];
    }
    ~Vector(){
        Reset();
    }
    
    
    int size() const
    {
        return(dim_);
    }
    int null() const
    {
        return dim_ == 0;
    }
    void sum(const Vector &A, const Vector &B) 
    {
        int n = min_(A.size(), B.size());
        for(int i=0; i<dim_ && i<n; i++) p_[i] = A[i] + B[i];
    }
    void sub(const Vector &A, const Vector &B) 
    {
        int n = min_(A.size(), B.size());
        for(int i=0; i<dim_ && i<n; i++) p_[i] = A[i] - B[i];
    }
    void mul_3(const Vector &A, const Vector &B) 
    {
        if(A.size() < 3 || B.size() < 3 || dim_ < 3) 
        {
            cout << "mul_3: dimention of the vectors must not less than 3!"
                << endl;
            fail_exit(1);
        }
        p_[0] = A[1]*B[2] - B[1]*A[2];
        p_[1] = A[2]*B[0] - B[2]*A[0];
        p_[2] = A[0]*B[1] - B[0]*A[1];
    }

    
    T&  operator[] (int i) const 
    {
        return p_[i];
    }
    Vector& operator=(const Vector &B)
    {
        for(int i=0; i<dim_ && i<B.size(); i++) p_[i] = B[i];
        return *this;
    }
    Vector& operator=(T a)
    {
        for(int i=0; i<dim_; i++) p_[i] = a; 
        return *this;
    }

    Vector operator+(const Vector &B);

    Vector operator-(const Vector &B);

    T operator*(const Vector &B);
    T operator*(const T a);
};

template <class T>
Vector<T> Vector<T>::operator+(const Vector<T> &B) 
{
    Vector E(dim_);
    for(int i=0; i<dim_ && i<B.size(); i++) E.p_[i] = p_[i] + B[i];
    return E;
}


template <class T>
Vector<T> Vector<T>::operator-(const Vector<T> &B) 
{
    Vector E(dim_);
    for(int i=0; i<dim_ && i<B.size(); i++) E.p_[i] = p_[i] - B[i];
    return E;
}

template <class T>
T Vector<T>::operator*(const Vector<T> &B) 
{
    int n = 0;
    T d;
    d = n;
    for(int i=0; i<dim_ && i<B.size(); i++) d += p_[i]*B[i];
    return(d);
}


template <class T>
T Vector<T>::operator*(const T a) 
{
    int n = 0;
    T d;
    d = n;
    for(int i=0; i<dim_ ; i++) d += p_[i]*a;
    return(d);
}


template <class T>
class Matrix {

protected:
    Vector<T> v_;
    int dim_[2]; 

public:
    
    Matrix() : v_() 
    {
        dim_[0] = 0;
        dim_[1] = 0;
    }
    Matrix(int nr, int nc) : v_(nr*nc) 
    {
        dim_[0] = nr;
        dim_[1] = nc;
    }
    Matrix(int nr, int nc, T s) : v_(nr*nc, s) 
    {
        dim_[0] = nr;
        dim_[1] = nc;
    }
    Matrix(Vector<T> &B, int nr, int nc) : v_(B, nr*nc)
    {
        dim_[0] = nr;
        dim_[1] = nc;
    }
    Matrix(Matrix &B, int nr, int nc) : v_(B.v_, nr*nc)
    {
        dim_[0] = nr;
        dim_[1] = nc;
    }
    ~Matrix()
    {
        dim_[0] = dim_[1] = 0;
    }

    
    int size() const
    {
        return(dim_[0]*dim_[1]);
    }
    int null() const
    {
        return(dim_[0] == 0 || dim_[1] == 0);
    }
    void sum(const Matrix &A, const Matrix &B)
    {
        int n = min_(A.size(), B.size()), m = dim_[0]*dim_[1];
        for(int i=0; i<m && i<n; i++) this->v_[i] = A[i] + B[i];
    }
    void sub(const Matrix &A, const Matrix &B) {
        int n = min_(A.size(), B.size()), m = dim_[0]*dim_[1];
        for(int i=0; i<m && i<n; i++) this->v_[i] = A[i] - B[i];
    }

    void mul(const Matrix &, const Matrix &);

    void mul_trans(const Matrix &, const Matrix &);

    
    T& operator[](int i) const
    {
        return (v_[i]);
    }
    T& operator()(int i, int j) const
    {
        return (v_[i*dim_[1] + j]);  
    }
    Matrix& operator=(const Matrix &B) {  
        for(int i=0; i<dim_[0]*dim_[1] && i<B.size(); i++) v_[i] = B[i];
        return(*this);
    }
    Matrix& operator=(T a) {
        for(int i=0; i<dim_[0]*dim_[1]; i++) v_[i] = a; 
        return(*this);
    }
};


template <class T>
void Matrix<T>::mul(const Matrix<T> &A, const Matrix<T> &B) {
    assert(dim_[0] <= A.dim_[0]);
    assert(dim_[1] <= B.dim_[1]);
    int n = min_(A.dim_[1], B.dim_[0]), i, j ,k, l = 0, m;
    T dt;
    for(i=0; i<dim_[0]; i++) {
        for(j=0; j<dim_[1]; j++) {
            m = 0;
            dt = m;
            for(k=0; k<n; k++) dt += A(i, k)*B(k, j);
            v_[l] = dt;
            l++;
        }
    }
}


template <class T>
void Matrix<T>::mul_trans(const Matrix<T> &A, const Matrix<T> &B) {
    assert(dim_[0] <= A.dim_[0]);
    assert(dim_[1] <= B.dim_[0]);
    int n = min_(A.dim_[1], B.dim_[1]), i, j ,k, l = 0, m;
    T dt;
    for(i=0; i<dim_[0]; i++) {
        for(j=0; j<dim_[0]; j++) {
            m = 0;
            dt = m;
            for(k=0; k<n; k++) dt += A(i, k)*B(j, k);
            v_[l] = dt;
            l++;
        }
    }
}


template <class T>
void Tfactor_usy(T *Coef, const int& n_row_col)
{
    int i, j, k, idia, i1, i0, j0;
    T C_dia, C_ji;
   
    i0 = 0;  
    for(i=0; i<n_row_col - 1; i++) 
    {
        idia = i0 + i;     
        C_dia = Coef[idia];
        i1 = i + 1;
        j0 = i1*n_row_col; 
        for(j=i1; j<n_row_col; j++) 
        {
            Coef[j0 + i] /= C_dia;
            C_ji = Coef[j0 + i];
            for(k=i1; k<n_row_col; k++) 
            {
                Coef[j0 + k] -= Coef[i0 + k]*C_ji;
            }
            j0 += n_row_col;
        }
        for(k=i1; k<n_row_col; k++) Coef[i0 + k] /= C_dia;
        i0 += n_row_col;
    }   
}


template <class T>
void Tfbelim(T *Coef, T *rhsv, const int& n_row_col)
{
    int i, j, i0;

    
    i0 = n_row_col;
    for(i=1; i<n_row_col; i++) 
    {
        for(j=0; j<i; j++)     
        {
            rhsv[i] -= Coef[i0 + j]*rhsv[j];
        }
        i0 += n_row_col;
    }

    for(i=0; i<n_row_col; i++) rhsv[i] /= Coef[i*n_row_col + i];

    
    for(i=n_row_col - 1; i>0; i--) 
    { 
        for(j=0; j<i; j++) rhsv[j] -= rhsv[i]*Coef[j*n_row_col + i];
    }
}


struct ElCluster
{
    int NuElGr;            
    int NumOfEl;           
    int listElementPropertyEl;          
                           
                           
    bool IfMonoEl;         
                           
    bool IfAdmiBc;         
    int NumOfDOFs;         
    int NDOFsPeEl;         
    int *NumsOfEl;         
    double CoorCent[3];	   
    double RadiClus;       
    int NumNeaClus;        
    int NumFarClus;        
    int *NumsNeaClus;      
    int *NumsFarClus;      
    int OriClust;          
    int nuref;             
    int rffac;             
    bool ifmirro;          
                           
    bool ifrfdi[3];        
                           
                           
    int nuFather;          
    int n_Son;             
    int nuSon[8];          
    int nuLev;             
    int NumFanClus;        
                           
                           
    int *NumsFanClus;      
};


struct IpCluster
{
    int NumOfIps;          
    int *NumsOfIps;        
    double CoorCent[3];	   
    double RadiClus;       
};


struct ClusterLev
{
    
    int nClustSLv;         
    int nClustOLv;         
    double RadiMaxLv;      
    double RadiAveLv;      
    double RadiMinLv;      
    int nExpaTermLv;       
    int nPoinSpheLv;       
    int nPoinThetLv;       
    int nPoinPhiLv;        
    int Pow_532Lv[3];      
                           
    int iplacsum;          
    int **iplacpova;       
    int **iregister;       
    double *CrdGauLv;      
    double *WeiGauLv;      
    double *XvThetLv;      
    double *XvPhiLv;       
    double *CmThetLv;      
                           
    double *CmPhiLv;	   
                           
    double *LmThetLv;      
    Complex *ZmThetLv;     
    Complex *ZmPhiLv;      
    Complex *PbarnmLv;     
    Complex *PbarfaLv;     
    Complex *zwkT;		   
    Complex *zwkS;		   
    double **uvcsphe;      
    double *weisphe;       
    Complex **zIntpMtx;    
                           
    Complex **zFiltMtx;    
    ElCluster *ClastArLv;  
};


struct D_mtx_lev
{
    int nRowsD;			
    int nEntriesD;		
    int *jcolDmxLv;		
    int *irowDmxLv;		
    Complex *zDmxLv;	
};


struct T_mtx_lev
{
    int nRowsT;			
    int nEntriesT;		
    int *jcolTmxLv;		
    int *irowTmxLv;		
    Complex *zTmxLv;	
    Complex *zTvcLv;    
};


struct S_mtx_lev
{
    int nRowsS;			
    int nEntriesS;		
    int *jcolSmxLv;		
    int *irowSmxLv;		
    Complex *zSmxLv;	
};


struct inputLineBoundaryCondition
{
    int		nLow,		
            nHigh;
    
    string	sKeyword;	
    double	dReal,		
            dImag,	
            dNegReal,	
            dNegImag;
    bool	bRef;		
    bool    bNega;      
    int		nRealRef,	
            nImagRef,
            nNegRealRef,
            nNegImagRef;
};



extern void NC_ReflectElementFMM(int&, bool&, Vector<bool>&,
                       const int&, const int&, Vector<double>&, 
                       Vector<double>&, Vector<Complex>&, Matrix<double>&);
extern void NC_ComputeGlobalCoordinates(Vector<double>&, Vector<double>&, Matrix<double>&,
                     const double&, const double&, const int&);
extern double NC_ComputeUnitVectorOnElement(Vector<double>&, Vector<double>&, Vector<double>&,
                       Vector<double>&, Vector<double>&, Matrix<double>&,
                       const double&, const double&, const int&);
extern double NC_ReflectPointVector(Vector<double>&, const int&, double&,
                      const int&);
extern void NC_ComputeElementData(const int&, const int&,
                    int&, int&, int&,
                    Vector<int>&, Vector<double>&, Vector<double>&,
                    Vector<Complex>&, Vector<Complex>&, Matrix<double>&); 
extern void NC_ReflectElement(const int&, const int&, const int&,
                    int&, Vector<double>&, Vector<double>&,
                    Vector<Complex>&, Matrix<double>&);
extern int NC_ComputeGausseanPoints(double*, double*, double*,
                    const int&, const int&);
extern double NC_ComputeParameters(Vector<double>&, Vector<double>&, Vector<double>&,
                      Vector<double>&, Vector<double>&, Vector<double>&,
                      Vector<double>&, Complex&, Matrix<double>&,
                      const int&, Vector<Complex>&, const int&,
                      const double&, const double&);
extern int NC_GenerateSubelements(ofstream&, const int&, Vector<double>&, const int&,
                    Matrix<double>&, const double&, const int&,
                    Vector<double>&, Vector<double>&, const int&,
                    Vector<double>&, Vector<double>&, Vector<double>&,
                    Vector<int>&);
void NC_ComputeUnitVectorOnUnitSphere(ofstream&, const int&, const int&, double*, double**);
void NC_SphericalHankel(ofstream&, Vector<Complex>&, const int&,const double&);
void NC_LegendrePolynomes(ofstream&, Vector<double>&, const int&,const double&);
extern void NC_Error_Exit_0(ofstream&, const string&);
extern void NC_Error_Warning_0(ofstream&, const string&);
extern void NC_Error_Exit_0(const string&);
extern void NC_Error_Warning_0(const string&);
void NC_Error_Exit_1(ofstream&, const string&, const string&,const int&);
void NC_Error_Exit_1(ofstream&, const string&, const string&,const double&);
void NC_Error_Exit_1(ofstream&, const string&, const string&);
void NC_Error_Warning_1(ofstream&, const string&, const string&,const int&);
void NC_Error_Warning_1(ofstream&, const string&, const string&,const double&);
void NC_Error_Exit_1(const string&, const string&, const int&);
void NC_Error_Exit_1(const string&, const string&, const double&);
void NC_Error_Exit_1(const string&, const string&);
void NC_Error_Warning_1(const string&, const string&, const int&);
void NC_Error_Warning_1(const string&, const string&, const double&);
void NC_Error_Exit_2(ofstream&, const string&, const string&,const int&, const string&, const int&);
void NC_Error_Exit_2(ofstream&, const string&, const string&,const double&, const string&, const double&);
void NC_Error_Warning_2(ofstream&, const string&, const string&,const int&, const string&, const int&);
void NC_Error_Warning_2(ofstream&, const string&, const string&,const double&, const string&, const double&);
void NC_Error_Warning_2(ofstream&, const string&, const string&,const int&, const string&, const double&);
void NC_Error_Exit_2(const string&, const string&, const int&,const string&, const int&);
void NC_Error_Exit_2(const string&, const string&, const double&,const string&, const double&);
void NC_Error_Warning_2(const string&, const string&, const int&,const string&, const int&);
void NC_Error_Warning_2(const string&, const string&, const double&,const string&, const double&);
extern ostream& operator<<(ostream &s, const Complex &u);
extern double BLdispoi(Vector<double>&, Vector<double>&);
extern Complex BLzsqrt(const Complex&);
void BLGauCooWe(ofstream&, const int&, Vector<double>&,Vector<double>&);
void NC_IterativeSolverCGS(ofstream&);
Vector<double> BLAverageVc_3(Vector<double>&, Vector<double>&, Vector<double>&);
double interpo_line(double&, double freq_poi[], double fac1_poi[], int&);



#endif
