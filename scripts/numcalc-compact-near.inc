// Body of the pinned NC_AllocateNmtx. The generator qualifies job state,
// accounts allocations and inserts cancellation checks as for upstream code.
// All original coefficient rows retain their offsets and summation order.
Vector<int> starts(numOriginalClusters_ + 1, 0);
Vector<bool> adjacent(numOriginalClusters_);
auto mark = [&](int cluster) {
    adjacent = false;
    const auto &c = ClustArray[cluster];
    for (int k=0; k<c.NumNeaClus; ++k) {
        adjacent[ClustArray[c.NumsNeaClus[k]].OriClust] = true;
    }
};
for (int c=0; c<numOriginalClusters_; ++c) {
    mark(c);
    int width = 0;
    for (int j=0; j<numOriginalClusters_; ++j) {
        if (adjacent[j]) width += ClustArray[j].NumOfDOFs;
    }
    if (width > INT_MAX - starts[c]) throw Failure(3,"Near field column count exceeds limit");
    starts[c+1] = starts[c]+width;
}
jcolnea = new int[starts[numOriginalClusters_]];
irownea = new int[numRowsOfCoefficientMatrix_+1];
nearColumnBias = new int[numRowsOfCoefficientMatrix_];
std::fill_n(nearColumnBias,numRowsOfCoefficientMatrix_,-1);
for (int c=0; c<numOriginalClusters_; ++c) {
    mark(c);
    int dst=starts[c];
    // The upstream CSR orders original neighbors by their ID, not in the
    // order of NumsNeaClus. Reflections of the same neighbor count once.
    for (int j=0; j<numOriginalClusters_; ++j) {
        if (!adjacent[j]) continue;
        const auto &n=ClustArray[j];
        for (int k=0; k<n.NumOfEl; ++k) {
            const int el=n.NumsOfEl[k];
            for (int d=0; d<n.NDOFsPeEl; ++d) {
                if (dst >= starts[c+1]) throw Failure(1,"Invalid cluster DOF count");
                jcolnea[dst++]=jelist[el][d];
            }
        }
    }
    if (dst != starts[c+1]) throw Failure(1,"Invalid near field column count");
    const auto &owner=ClustArray[c];
    for (int k=0; k<owner.NumOfEl; ++k) {
        for (int d=0; d<owner.NDOFsPeEl; ++d) {
            const int row=jelist[owner.NumsOfEl[k]][d];
            if (row<0 || row>=numRowsOfCoefficientMatrix_ || nearColumnBias[row]!=-1)
                throw Failure(1,"Invalid near field row owner");
            nearColumnBias[row]=c;
        }
    }
}
numNonZeroEntries_=0;
for (int row=0; row<numRowsOfCoefficientMatrix_; ++row) {
    const int c=nearColumnBias[row];
    if (c<0) throw Failure(1,"Missing near field row owner");
    irownea[row]=numNonZeroEntries_;
    nearColumnBias[row]=starts[c]-numNonZeroEntries_;
    const int width=starts[c+1]-starts[c];
    if (width > INT_MAX-numNonZeroEntries_) throw Failure(3,"Near field exceeds solver dimensions");
    numNonZeroEntries_+=width;
}
irownea[numRowsOfCoefficientMatrix_]=numNonZeroEntries_;
boolComputeTVector_=false;
for (int c=0; c<numOriginalReflectedClusters_; ++c) {
    if (ClustArray[c].listElementPropertyEl==0 && ibval[ClustArray[c].NumsOfEl[0]]<3) {
        boolComputeTVector_=true; break;
    }
}
