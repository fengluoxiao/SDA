#pragma once
#include <Eigen/Core>
#include <Eigen/Geometry>
#include <cmath>

// Only an initial guess for ICP. Never a measured/accepted pose on its own.
inline bool sdaPredictTrackingPose(const Eigen::Matrix4f& older, const Eigen::Matrix4f& last,
                                  double olderTime, double lastTime, double now,
                                  Eigen::Matrix4f& prediction) {
    if (!std::isfinite(olderTime) || !std::isfinite(lastTime) || !std::isfinite(now)) return false;
    const double dt=lastTime-olderTime, ahead=now-lastTime;
    if (olderTime < 0 || dt < 0.01 || dt > 0.2 || ahead <= 0 || ahead > 0.2 || ahead/dt > 2) return false;
    auto rigid=[](const Eigen::Matrix4f& m) {
        if (!m.allFinite() || (m.row(3)-Eigen::RowVector4f(0,0,0,1)).squaredNorm()>1e-8f) return false;
        const Eigen::Matrix3f rot=m.block<3,3>(0,0);
        return (rot.transpose()*rot-Eigen::Matrix3f::Identity()).squaredNorm()<1e-5f && std::abs(rot.determinant()-1)<0.001f;
    };
    if (!rigid(older) || !rigid(last)) return false;
    Eigen::Quaternionf q0(older.block<3,3>(0,0)), q1(last.block<3,3>(0,0));
    q0.normalize(); q1.normalize();
    Eigen::AngleAxisf turn(q0.conjugate()*q1);
    const float ratio=float(ahead/dt), angle=turn.angle()*ratio;
    const Eigen::Vector3f step=(last.block<3,1>(0,3)-older.block<3,1>(0,3))*ratio;
    if (!std::isfinite(angle) || !step.allFinite() || angle>0.12f || step.norm()>0.025f) return false;
    prediction=last;
    prediction.block<3,3>(0,0)=(q1*Eigen::Quaternionf(Eigen::AngleAxisf(angle,turn.axis()))).normalized().toRotationMatrix();
    prediction.block<3,1>(0,3)+=step;
    return rigid(prediction);
}
